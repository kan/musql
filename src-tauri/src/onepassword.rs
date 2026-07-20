//! 1Password CLI (`op`) integration.
//!
//! 1Password is used as a *source* for credentials, never as a runtime dependency of the
//! connection path: a secret is fetched once, stored in the OS keyring, and from then on
//! the keyring answers. `op` is only invoked when the keyring has nothing (first use on a
//! machine that received the profile through sync) or when the user explicitly asks for a
//! refresh. That keeps the biometric-approval prompt off the hot path — on Windows the
//! 1Password desktop app re-authorizes per calling process, so invoking `op` on every
//! connection would mean a Windows Hello prompt every time.
//!
//! There is no official 1Password SDK for Rust, so this shells out to the CLI.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

// Generous: the call may be blocked on the user approving a Windows Hello prompt. The
// timeout exists only so a wedged `op` cannot pin a blocking thread forever.
const OP_TIMEOUT: Duration = Duration::from_secs(90);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Cached so that a fetch costs one process rather than two — the lookup itself runs
/// `op --version`. Whether `op` is installed does not change within a session.
static OP_BINARY: OnceLock<Option<String>> = OnceLock::new();

/// Locate `op`. PATH covers the normal install (winget puts its shim there and a GUI app
/// launched from Explorer inherits the user environment); the explicit paths are a
/// fallback for the case where it does not.
fn op_binary() -> Option<String> {
    OP_BINARY
        .get_or_init(|| {
            if run_raw("op", &["--version"]).is_ok() {
                return Some("op".to_string());
            }
            let candidates = [
                std::env::var("LOCALAPPDATA")
                    .ok()
                    .map(|p| format!("{p}\\Microsoft\\WinGet\\Links\\op.exe")),
                std::env::var("ProgramFiles")
                    .ok()
                    .map(|p| format!("{p}\\1Password CLI\\op.exe")),
            ];
            candidates
                .into_iter()
                .flatten()
                .find(|p| std::path::Path::new(p).exists())
        })
        .clone()
}

/// Run a command to completion with a timeout, returning stdout on success and the
/// process's stderr on failure. stdin is closed so `op` can never sit waiting on input.
fn run_raw(program: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Without this a console window flashes up on every call.
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to run {program}: {e}"))?;

    // Back off from a short first interval so `op --version` (the availability probe)
    // does not pay a fixed sleep, while a slow `op read` still polls cheaply.
    let start = Instant::now();
    let mut interval = Duration::from_millis(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() > OP_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait(); // reap; Child does not do this on drop
                    return Err("1Password CLI timed out".to_string());
                }
                std::thread::sleep(interval);
                interval = (interval * 2).min(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Failed to wait for {program}: {e}")),
        }
    };

    // Read after exit. `op` writes far less than a pipe buffer holds, so it cannot block
    // on a full pipe before exiting; if it ever did, the timeout above bounds the wait.
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }

    if status.success() {
        Ok(stdout)
    } else {
        let msg = stderr.trim();
        Err(if msg.is_empty() {
            format!("{program} exited with {status}")
        } else {
            msg.to_string()
        })
    }
}

/// Reject anything that is not a secret reference. `op` is invoked with an argument
/// vector rather than a shell, so this is not injection defense — it is to turn a
/// mistyped or stale value into a clear message instead of an opaque CLI error.
fn validate_reference(reference: &str) -> Result<&str, String> {
    let r = reference.trim();
    if r.is_empty() {
        return Err("1Password reference is empty".to_string());
    }
    if !r.starts_with("op://") {
        return Err(
            "1Password reference must start with op:// (copy it from 1Password with \
             \"Copy Secret Reference\")"
                .to_string(),
        );
    }
    if r.contains(['\n', '\r']) {
        return Err("1Password reference contains a line break".to_string());
    }
    Ok(r)
}

/// Whether the CLI is installed. Says nothing about whether it is signed in.
pub fn is_available() -> bool {
    op_binary().is_some()
}

/// Resolve a secret reference to its value. Blocking — call from a blocking context.
pub fn read_secret(reference: &str) -> Result<String, String> {
    let reference = validate_reference(reference)?;
    let bin = op_binary().ok_or_else(|| {
        "1Password CLI (op) not found. Install it and enable \"Integrate with 1Password CLI\" \
         in the 1Password desktop app."
            .to_string()
    })?;
    let value = run_raw(&bin, &["read", "--no-newline", reference])?;
    if value.is_empty() {
        return Err(format!("1Password returned an empty value for {reference}"));
    }
    Ok(value)
}

/// Best-effort variant for the connection path: `None` when there is no reference to
/// resolve, so callers can distinguish "not configured" from "configured but failed".
pub fn read_optional(reference: Option<&String>) -> Option<Result<String, String>> {
    let r = reference.map(|s| s.trim()).filter(|s| !s.is_empty())?;
    Some(read_secret(r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_reference_accepts_secret_reference() {
        assert_eq!(
            validate_reference("op://Private/prod-db/password").unwrap(),
            "op://Private/prod-db/password"
        );
    }

    #[test]
    fn validate_reference_trims() {
        assert_eq!(
            validate_reference("  op://Private/db/pw  ").unwrap(),
            "op://Private/db/pw"
        );
    }

    #[test]
    fn validate_reference_rejects_plain_text() {
        assert!(validate_reference("prod-db/password").is_err());
        assert!(validate_reference("").is_err());
        assert!(validate_reference("   ").is_err());
    }

    #[test]
    fn validate_reference_rejects_line_break() {
        assert!(validate_reference("op://Private/db/pw\nrm -rf").is_err());
    }

    // Exercises the real CLI end to end (binary lookup, spawn, stderr capture). Requires
    // `op` installed and signed in, so it is opt-in: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn read_secret_surfaces_cli_error() {
        let err = read_secret("op://NoSuchVault/NoSuchItem/password").unwrap_err();
        assert!(err.contains("isn't a vault"), "unexpected error: {err}");
    }

    #[test]
    fn read_optional_none_when_unset() {
        assert!(read_optional(None).is_none());
        assert!(read_optional(Some(&String::new())).is_none());
        assert!(read_optional(Some(&"   ".to_string())).is_none());
    }
}
