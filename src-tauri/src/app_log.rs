//! Application log file (#110).
//!
//! Written on installed builds too: a release build cannot open DevTools, so without a
//! file there is nothing to read after a connection failure or a JS error.
//!
//! - Location: the OS log directory (`app_log_dir`, i.e.
//!   `%LOCALAPPDATA%\{identifier}\logs` on Windows). Dev builds use a `.debug`
//!   identifier, so they never share a file with the installed app.
//! - Stdout is a target on dev builds only (`just dev` keeps printing to the terminal).
//! - Uncaught frontend errors arrive through `log_frontend`. Throttling happens on the
//!   JS side (`ui/error-log.js`).
//!
//! **What must never be logged:** passwords, passphrases, API keys, SQL text and query
//! results. Query execution errors are not logged either, because MySQL quotes a piece
//! of the statement in them. Connection errors are logged; they name the host and the
//! user, which is what makes them useful.
//!
//! Several instances may run at once (by design, see `.claude/rules/rust.md`) and they
//! all append to the same file. Lines can interleave, and each process counts the size
//! on its own, so rotation may happen a little late. That is accepted: the file is a
//! diagnostic aid, not a record.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, Window};
use tauri_plugin_log::{Builder, RotationStrategy, Target, TargetKind, TimezoneStrategy};

/// Repeat window for failures a user can trigger in bursts (a broken connection makes
/// every query of a tab fail at once).
pub const BRIEF: Duration = Duration::from_secs(60);

/// Repeat window for a condition that stays true for hours and is re-checked on every
/// window focus (an unreachable sync file).
pub const LONG: Duration = Duration::from_secs(3600);

/// Messages remembered for throttling. Cleared wholesale when exceeded; the cost is at
/// most one repeated line per message.
const MAX_THROTTLE_KEYS: usize = 200;

static RECENT_WARNINGS: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// Decides whether `message` should be written now, and records it if so.
fn should_log(
    seen: &mut HashMap<String, Instant>,
    message: &str,
    now: Instant,
    window: Duration,
) -> bool {
    if let Some(last) = seen.get(message) {
        if now.duration_since(*last) < window {
            return false;
        }
    }
    if seen.len() >= MAX_THROTTLE_KEYS {
        seen.clear();
    }
    seen.insert(message.to_owned(), now);
    true
}

/// `log::warn!` that skips a message identical to one written within `window`. Without
/// it a single lasting condition fills the file and rotates the useful lines away.
pub fn warn_throttled(window: Duration, message: &str) {
    let write = match RECENT_WARNINGS.lock() {
        Ok(mut guard) => should_log(
            guard.get_or_insert_with(HashMap::new),
            message,
            Instant::now(),
            window,
        ),
        // A poisoned lock must not silence logging.
        Err(_) => true,
    };
    if write {
        log::warn!("{message}");
    }
}

/// Size at which the file is rotated. The plugin default (40KB) turns over after a few
/// stack traces.
const MAX_FILE_SIZE: u128 = 1_000_000;

/// Rotated files to keep. The default `KeepOne` deletes the previous log the moment it
/// rotates, which loses the evidence if that happens right after a failure.
const KEEP_ROTATED: usize = 2;

/// Log file name (the plugin appends `.log`).
const FILE_NAME: &str = "muSQL";

/// Upper bound for one frontend message, so a huge object cannot bloat a line.
const MAX_FRONTEND_MESSAGE: usize = 4_000;

/// Registers the log plugin. Called first thing in `setup`.
///
/// A failure here must not stop the app: the plugin errors when it cannot create the
/// directory, open the file or rotate, and a diagnostics feature that prevents startup
/// would be worse than no log.
pub fn init(app: &AppHandle) {
    if let Err(e) = app.plugin(plugin()) {
        eprintln!("[app_log] logging disabled: {e}");
        return;
    }
    log_panics();
    log::info!(
        "muSQL {} started ({})",
        app.package_info().version,
        app.config().identifier
    );
    // Where the file really is; on the Store build this is not `app_log_dir` (#124).
    if let Ok(dir) = app.path().app_log_dir() {
        log::info!("log directory: {}", real_log_dir(&dir).display());
    }
}

/// Also record panics in the log. Stderr is not read by anyone on an installed build.
/// The default hook stays chained behind and still prints the full message there.
///
/// **Only the location is written, not the message.** A panic message is whatever an
/// `unwrap()` / `expect()` formatted, which can be a MySQL error (with SQL in it) or row
/// data. The location is enough to find the line.
fn log_panics() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        match info.location() {
            Some(loc) => log::error!("panic at {}:{}:{}", loc.file(), loc.line(), loc.column()),
            None => log::error!("panic at an unknown location"),
        }
        default_hook(info);
    }));
}

fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let mut targets = vec![Target::new(TargetKind::LogDir {
        file_name: Some(FILE_NAME.to_owned()),
    })];
    if cfg!(debug_assertions) {
        targets.push(Target::new(TargetKind::Stdout));
    }
    Builder::new()
        .targets(targets)
        // Dependencies log from Warn up; at Info an idle installed app would still grow
        // the file. muSQL's own `log::info!` lines are kept.
        .level(log::LevelFilter::Warn)
        .level_for("musql", log::LevelFilter::Info)
        .max_file_size(MAX_FILE_SIZE)
        .rotation_strategy(RotationStrategy::KeepSome(KEEP_ROTATED))
        // Local time, so a user can match a line against when something happened.
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .build()
}

/// Cuts a frontend message to `MAX_FRONTEND_MESSAGE` bytes on a char boundary.
fn clamp_message(message: &str) -> &str {
    if message.len() <= MAX_FRONTEND_MESSAGE {
        return message;
    }
    let mut end = MAX_FRONTEND_MESSAGE;
    while !message.is_char_boundary(end) {
        end -= 1;
    }
    &message[..end]
}

/// Indents continuation lines, so a multi-line stack reads as one entry and text from
/// the webview can never pass for a line of its own.
fn indent_continuation(message: &str) -> String {
    message.replace('\r', "").replace('\n', "\n    ")
}

/// Writes an uncaught frontend error to the log, prefixed with the window it came from.
#[tauri::command]
pub fn log_frontend(window: Window, message: String) {
    let message = indent_continuation(clamp_message(&message));
    log::error!(target: "webview", "[{}] {message}", window.label());
}

/// Turns the `\\?\` form that `canonicalize` returns on Windows back into a path the
/// shell accepts (`\\?\C:\x` → `C:\x`, `\\?\UNC\host\share` → `\\host\share`).
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{unc}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

/// The directory the log file is really in (#124).
///
/// The Store (MSIX) build runs packaged: Windows redirects what it creates under
/// `%LOCALAPPDATA%` to `…\Packages\<package>\LocalCache\Local\…`. Inside the app both
/// paths work, but Explorer is outside the package and only sees the real one, so
/// handing it `app_log_dir` opens an empty folder. `canonicalize` asks Windows for the
/// final path of an existing file, which is the redirected one.
///
/// The file is resolved before the directory: redirection is per file, and a directory
/// left behind by the installer build can be real while the new log inside it is not.
fn real_log_dir(dir: &Path) -> PathBuf {
    std::fs::canonicalize(dir.join(format!("{FILE_NAME}.log")))
        .ok()
        .and_then(|file| file.parent().map(Path::to_path_buf))
        .or_else(|| std::fs::canonicalize(dir).ok())
        .map(|real| strip_verbatim_prefix(&real))
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Opens the log directory in the file manager. Created first, so it opens even before
/// the first line is written.
pub fn open_dir(app: &AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_path(real_log_dir(&dir), None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_message_keeps_short_messages() {
        assert_eq!(
            clamp_message("TypeError: x is undefined"),
            "TypeError: x is undefined"
        );
    }

    #[test]
    fn should_log_skips_repeats_inside_the_window() {
        let mut seen = HashMap::new();
        let t0 = Instant::now();
        assert!(should_log(&mut seen, "connection failed", t0, BRIEF));
        assert!(!should_log(
            &mut seen,
            "connection failed",
            t0 + Duration::from_secs(59),
            BRIEF
        ));
        // A different message is not held back by the first one.
        assert!(should_log(
            &mut seen,
            "sync failed",
            t0 + Duration::from_secs(1),
            BRIEF
        ));
        assert!(should_log(
            &mut seen,
            "connection failed",
            t0 + Duration::from_secs(60),
            BRIEF
        ));
    }

    #[test]
    fn should_log_stays_bounded() {
        let mut seen = HashMap::new();
        let now = Instant::now();
        for i in 0..(MAX_THROTTLE_KEYS * 3) {
            assert!(should_log(&mut seen, &format!("message {i}"), now, BRIEF));
            assert!(seen.len() <= MAX_THROTTLE_KEYS);
        }
    }

    #[test]
    fn indent_continuation_keeps_webview_text_off_column_zero() {
        assert_eq!(
            indent_continuation("TypeError: x\r\n    at f\n[2026-10-08][ERROR] forged"),
            "TypeError: x\n        at f\n    [2026-10-08][ERROR] forged"
        );
    }

    #[test]
    fn strip_verbatim_prefix_gives_a_path_the_shell_accepts() {
        let strip = |s: &str| {
            strip_verbatim_prefix(Path::new(s))
                .to_string_lossy()
                .into_owned()
        };
        assert_eq!(strip(r"\\?\C:\Users\me\logs"), r"C:\Users\me\logs");
        assert_eq!(strip(r"\\?\UNC\host\share\logs"), r"\\host\share\logs");
        assert_eq!(strip(r"C:\Users\me\logs"), r"C:\Users\me\logs");
    }

    #[test]
    fn real_log_dir_resolves_an_existing_directory_and_falls_back_otherwise() {
        let existing = std::env::temp_dir();
        let resolved = real_log_dir(&existing);
        assert!(resolved.is_dir());
        assert!(!resolved.to_string_lossy().starts_with(r"\\?\"));

        let missing = existing.join("musql-no-such-log-dir-124");
        assert_eq!(real_log_dir(&missing), missing);
    }

    #[test]
    fn clamp_message_cuts_on_a_char_boundary() {
        // 3-byte chars put the limit inside a character.
        let long = "あ".repeat(MAX_FRONTEND_MESSAGE);
        let cut = clamp_message(&long);
        assert!(cut.len() <= MAX_FRONTEND_MESSAGE);
        assert!(cut.len() > MAX_FRONTEND_MESSAGE - 3);
        assert!(cut.chars().all(|c| c == 'あ'));
    }
}
