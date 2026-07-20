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

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
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

    // Drain both pipes on their own threads. Waiting for exit before reading would
    // deadlock: a pipe buffer holds ~64 KB, and `op item list` emits several hundred KB,
    // so the child blocks mid-write and never exits while we poll for it to.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(pipe) = out_pipe.as_mut() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(pipe) = err_pipe.as_mut() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

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

    // The child has exited (or was killed), so both pipes are at EOF and these join
    // immediately.
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();

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

// ── Browsing (the reference picker) ──
//
// `op item get` returns each field's `value` alongside its metadata. None of these structs
// declare a `value`, so serde drops it while parsing and the secret never exists in a form
// that could be handed to the WebView. Do not add one.

#[derive(Debug, Clone, Serialize)]
pub struct OpItem {
    pub id: String,
    pub title: String,
    pub vault: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpField {
    pub label: String,
    pub reference: String,
    pub kind: String,
}

#[derive(Deserialize)]
struct RawVault {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawItem {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    vault: Option<RawVault>,
    #[serde(default)]
    category: String,
}

#[derive(Deserialize)]
struct RawField {
    #[serde(default)]
    label: String,
    // Supplied by `op` already assembled as op://vault/item/field — there is no need to
    // build the reference ourselves, and doing so would get quoting of odd names wrong.
    #[serde(default)]
    reference: Option<String>,
    #[serde(default, rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
struct RawItemDetail {
    #[serde(default)]
    fields: Vec<RawField>,
}

/// Listing every item is a slow network round trip, so it is fetched once per session.
/// Shared across windows because they are one process.
static ITEM_CACHE: Mutex<Option<Vec<OpItem>>> = Mutex::new(None);
/// Held across the CLI call (which ITEM_CACHE's lock deliberately is not) so that two
/// windows opening the picker at once do not both pay for the fetch.
static ITEM_FETCH: Mutex<()> = Mutex::new(());

fn cached_items() -> Option<Vec<OpItem>> {
    ITEM_CACHE.lock().ok().and_then(|cache| cache.clone())
}

fn run_op(args: &[&str]) -> Result<String, String> {
    let bin = op_binary().ok_or_else(|| {
        "1Password CLI (op) not found. Install it and enable \"Integrate with 1Password CLI\" \
         in the 1Password desktop app."
            .to_string()
    })?;
    run_raw(&bin, args)
}

/// Every item the account can see: id, title, vault and category only.
pub fn list_items(refresh: bool) -> Result<Vec<OpItem>, String> {
    if !refresh {
        if let Some(items) = cached_items() {
            return Ok(items);
        }
    }
    // Poisoning only means an earlier caller panicked; the data behind it is a `()`, so
    // there is nothing to be inconsistent about.
    let _fetching = ITEM_FETCH.lock().unwrap_or_else(|e| e.into_inner());
    // Another caller may have filled the cache while we waited for the lock.
    if !refresh {
        if let Some(items) = cached_items() {
            return Ok(items);
        }
    }
    let out = run_op(&["item", "list", "--format", "json"])?;
    let items = parse_items(&out)?;
    if let Ok(mut cache) = ITEM_CACHE.lock() {
        *cache = Some(items.clone());
    }
    Ok(items)
}

/// The referenceable fields of one item. Values are discarded during parsing.
pub fn list_fields(item_id: &str) -> Result<Vec<OpField>, String> {
    if item_id.is_empty() || !item_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err("Invalid 1Password item id".to_string());
    }
    let out = run_op(&["item", "get", item_id, "--format", "json"])?;
    parse_fields(&out)
}

fn parse_items(json: &str) -> Result<Vec<OpItem>, String> {
    let raw: Vec<RawItem> =
        serde_json::from_str(json).map_err(|e| format!("Failed to parse 1Password items: {e}"))?;
    Ok(raw
        .into_iter()
        .map(|it| OpItem {
            id: it.id,
            title: it.title,
            vault: it.vault.map(|v| v.name).unwrap_or_default(),
            category: it.category,
        })
        .collect())
}

fn parse_fields(json: &str) -> Result<Vec<OpField>, String> {
    let detail: RawItemDetail =
        serde_json::from_str(json).map_err(|e| format!("Failed to parse 1Password item: {e}"))?;
    Ok(detail
        .fields
        .into_iter()
        .filter_map(|f| {
            // A field without a reference cannot be pointed at, so it is not offered.
            let reference = f.reference?;
            Some(OpField {
                label: if f.label.is_empty() {
                    reference.rsplit('/').next().unwrap_or("").to_string()
                } else {
                    f.label
                },
                reference,
                kind: f.kind,
            })
        })
        .collect())
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

    // A pipe buffer holds ~64 KB. `op item list` emits several hundred KB, which used to
    // wedge the child mid-write while we polled for it to exit — the picker hit this as a
    // 90-second timeout every time. Anything at all above the buffer size reproduces it.
    #[cfg(windows)]
    #[test]
    fn run_raw_handles_output_larger_than_the_pipe_buffer() {
        let out = run_raw(
            "powershell",
            &["-NoProfile", "-Command", "Write-Output ('x' * 300000)"],
        )
        .expect("large output must not deadlock");
        assert!(
            out.trim().len() >= 300_000,
            "truncated output: {} bytes",
            out.trim().len()
        );
    }

    // Exercises the real CLI end to end (binary lookup, spawn, stderr capture). Requires
    // `op` installed and signed in, so it is opt-in: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn read_secret_surfaces_cli_error() {
        let err = read_secret("op://NoSuchVault/NoSuchItem/password").unwrap_err();
        assert!(err.contains("isn't a vault"), "unexpected error: {err}");
    }

    // The picker's whole security property: `op item get` hands back secret values, and
    // none of them may survive parsing into something the WebView could receive.
    #[test]
    fn parse_fields_drops_secret_values() {
        let json = r#"{
          "id": "abc", "title": "prod-db",
          "fields": [
            {"id": "username", "type": "STRING", "purpose": "USERNAME",
             "label": "username", "value": "SECRET-USERNAME",
             "reference": "op://Employee/prod-db/username"},
            {"id": "password", "type": "CONCEALED", "purpose": "PASSWORD",
             "label": "password", "value": "SECRET-PASSWORD",
             "reference": "op://Employee/prod-db/password",
             "password_details": {"strength": "FANTASTIC"}},
            {"id": "otp", "type": "OTP", "label": "one-time password",
             "value": "otpauth://totp/SECRET-SEED", "totp": "123456",
             "reference": "op://Employee/prod-db/one-time password"}
          ]
        }"#;
        let fields = parse_fields(json).unwrap();
        assert_eq!(fields.len(), 3);

        let rendered = serde_json::to_string(&fields).unwrap();
        assert!(!rendered.contains("SECRET"), "secret leaked: {rendered}");
        assert!(!rendered.contains("123456"), "totp leaked: {rendered}");

        assert_eq!(fields[1].label, "password");
        assert_eq!(fields[1].kind, "CONCEALED");
        assert_eq!(fields[1].reference, "op://Employee/prod-db/password");
    }

    #[test]
    fn parse_fields_skips_unreferenceable_fields() {
        let json = r#"{"fields": [
          {"label": "notes", "type": "STRING", "value": "no reference here"},
          {"label": "pw", "type": "CONCEALED", "reference": "op://v/i/pw"}
        ]}"#;
        let fields = parse_fields(json).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].reference, "op://v/i/pw");
    }

    #[test]
    fn parse_fields_falls_back_to_the_reference_tail_for_an_unlabelled_field() {
        let json = r#"{"fields": [{"type": "CONCEALED", "reference": "op://v/i/api key"}]}"#;
        assert_eq!(parse_fields(json).unwrap()[0].label, "api key");
    }

    #[test]
    fn parse_items_keeps_only_identifying_metadata() {
        let json = r#"[
          {"id": "aaa", "title": "prod-db", "category": "LOGIN",
           "vault": {"id": "v1", "name": "Employee"},
           "urls": [{"href": "https://example.com"}], "version": 3},
          {"id": "bbb", "title": "no vault", "category": "PASSWORD"}
        ]"#;
        let items = parse_items(json).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "prod-db");
        assert_eq!(items[0].vault, "Employee");
        assert_eq!(items[0].category, "LOGIN");
        // A missing vault must not drop the item from the picker.
        assert_eq!(items[1].vault, "");
    }

    #[test]
    fn list_fields_rejects_a_malformed_item_id() {
        // Guards the argument that is interpolated into the CLI invocation.
        assert!(list_fields("").is_err());
        assert!(list_fields("../../etc/passwd").is_err());
        assert!(list_fields("--format").is_err());
    }

    #[test]
    fn read_optional_none_when_unset() {
        assert!(read_optional(None).is_none());
        assert!(read_optional(Some(&String::new())).is_none());
        assert!(read_optional(Some(&"   ".to_string())).is_none());
    }
}
