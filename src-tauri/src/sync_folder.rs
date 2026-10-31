//! The file side of the profile sync (#143): finding the cloud-synced folders on this
//! machine, writing the sync file so a half-written file is never uploaded, and finding the
//! conflict copies the sync clients leave next to it. The merge itself lives in `main.rs`.

use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Where the sync file goes inside a detected folder. Every machine that picks the same
/// folder lands on the same file.
const SYNC_DIR: &str = "muSQL";
const SYNC_FILE: &str = "profiles-sync.json";

/// A synced folder found on this machine, offered as a sync destination.
#[derive(Debug, Serialize, PartialEq)]
pub struct SyncFolder {
    /// `onedrive` / `onedrive_business` / `dropbox` / `dropbox_business`. The UI names it.
    pub service: &'static str,
    /// The synced folder itself.
    pub folder: String,
    /// The sync file inside it.
    pub file: String,
    /// True when the sync file is already there (another machine set it up).
    pub exists: bool,
}

/// The synced folders on this machine, OneDrive first. Folders that do not exist are left
/// out (an environment variable can outlive an uninstalled client).
pub fn candidates() -> Vec<SyncFolder> {
    find(
        |key| std::env::var(key).ok(),
        |path| std::fs::read_to_string(path).ok(),
    )
    .into_iter()
    .filter(|(_, folder)| Path::new(folder).is_dir())
    .map(|(service, folder)| {
        let file = Path::new(&folder).join(SYNC_DIR).join(SYNC_FILE);
        SyncFolder {
            service,
            exists: file.is_file(),
            file: file.to_string_lossy().into_owned(),
            folder,
        }
    })
    .collect()
}

/// The folders named by the clients' own settings, deduplicated, in display order.
/// `env` reads an environment variable and `read` a file, so tests can supply both.
fn find(
    env: impl Fn(&str) -> Option<String>,
    read: impl Fn(&Path) -> Option<String>,
) -> Vec<(&'static str, String)> {
    let mut found: Vec<(&'static str, String)> = Vec::new();
    let mut add = |service: &'static str, folder: Option<String>| {
        let Some(folder) = folder.map(|f| f.trim().trim_end_matches(['\\', '/']).to_owned()) else {
            return;
        };
        if folder.is_empty() || found.iter().any(|(_, f)| f.eq_ignore_ascii_case(&folder)) {
            return;
        }
        found.push((service, folder));
    };

    // OneDrive sets these for the signed-in accounts. `OneDrive` is the default account and
    // usually repeats one of the other two, which the dedup drops.
    add("onedrive", env("OneDriveConsumer"));
    add("onedrive_business", env("OneDriveCommercial"));
    add("onedrive", env("OneDrive"));

    // Dropbox writes its folders to info.json, under LOCALAPPDATA in current versions and
    // APPDATA in older ones.
    for base in ["LOCALAPPDATA", "APPDATA"] {
        let Some(base) = env(base) else { continue };
        let Some(text) = read(&Path::new(&base).join("Dropbox").join("info.json")) else {
            continue;
        };
        let Ok(info) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        for (key, service) in [("personal", "dropbox"), ("business", "dropbox_business")] {
            add(service, info[key]["path"].as_str().map(str::to_owned));
        }
    }
    found
}

/// Writes `data` to `path` by way of a temporary file in the same folder, so the sync client
/// only ever sees the old file or the complete new one. The temporary name starts with `~`
/// and ends in `.tmp`, which Dropbox does not upload. Creates the folder if it is missing.
pub fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("the sync path has no file name"))?;
    let tmp = dir.join(format!(
        "~{}.{}-{}.tmp",
        name.to_string_lossy(),
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        replace(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Renames `from` over `to`. A sync client that is reading `to` at that moment holds it open
/// without delete sharing, which fails the rename, so retry a few times before giving up.
fn replace(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match std::fs::rename(from, to) {
            Err(_) if attempt < 3 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            result => return result,
        }
    }
}

/// The files next to `path` that look like conflict copies of it: OneDrive's
/// `profiles-sync-PC.json` and Dropbox's `profiles-sync (… conflicted copy …).json`. Only
/// the name is checked here; the caller confirms the content is a sync file.
pub fn conflict_copies(path: &Path) -> Vec<PathBuf> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let is_copy = conflict_copy_matcher(name);
    let mut copies: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| e.file_name().to_str().is_some_and(&is_copy))
        .map(|e| e.path())
        .collect();
    copies.sort();
    copies
}

/// Matches names that are `target` with a sync client's conflict marker inserted before the
/// extension. The match is narrow on purpose, because matching files are deleted after the
/// merge: a backup the user made by hand (`profiles-sync - コピー.json`,
/// `profiles-sync (1).json`, `profiles-sync-old.json`) must not match.
fn conflict_copy_matcher(target: &str) -> impl Fn(&str) -> bool {
    let (stem, ext) = target.rsplit_once('.').unwrap_or((target, ""));
    let (stem, ext) = (stem.to_owned(), format!(".{ext}"));
    move |candidate| {
        let Some(middle_end) = candidate.len().checked_sub(ext.len()) else {
            return false;
        };
        let (Some(head), Some(middle), Some(tail)) = (
            candidate.get(..stem.len()),
            candidate.get(stem.len()..middle_end),
            candidate.get(middle_end..),
        ) else {
            return false;
        };
        head.eq_ignore_ascii_case(&stem)
            && tail.eq_ignore_ascii_case(&ext)
            && (is_onedrive_marker(middle) || is_dropbox_marker(middle))
    }
}

/// OneDrive appends `-` and the PC name (`-DESKTOP-ABC123`). Windows PC names are upper
/// case letters, digits and hyphens, which tells them from a hand-made `-old` or `-2`.
fn is_onedrive_marker(middle: &str) -> bool {
    middle.strip_prefix('-').is_some_and(|name| {
        name.len() <= 24
            && name.chars().any(|c| c.is_ascii_uppercase())
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
    })
}

/// Dropbox appends ` (<who>'s conflicted copy <date>)`, or ` (<who> の競合コピー <date>)` in
/// Japanese.
fn is_dropbox_marker(middle: &str) -> bool {
    middle.starts_with(" (") && middle.ends_with(')') && {
        let lower = middle.to_lowercase();
        lower.contains("conflicted copy") || lower.contains("競合コピー")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn find_with(vars: &[(&str, &str)], files: &[(&str, &str)]) -> Vec<(&'static str, String)> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        let files: HashMap<PathBuf, String> = files
            .iter()
            .map(|(k, v)| (PathBuf::from(k), (*v).to_owned()))
            .collect();
        find(|k| vars.get(k).cloned(), |p| files.get(p).cloned())
    }

    #[test]
    fn onedrive_default_account_is_not_listed_twice() {
        let found = find_with(
            &[
                ("OneDriveConsumer", r"C:\Users\me\OneDrive"),
                ("OneDrive", r"C:\Users\me\OneDrive\"),
                ("OneDriveCommercial", r"C:\Users\me\OneDrive - Contoso"),
            ],
            &[],
        );
        assert_eq!(
            found,
            vec![
                ("onedrive", r"C:\Users\me\OneDrive".to_owned()),
                (
                    "onedrive_business",
                    r"C:\Users\me\OneDrive - Contoso".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn onedrive_only_default_variable() {
        let found = find_with(&[("OneDrive", r"D:\OneDrive")], &[]);
        assert_eq!(found, vec![("onedrive", r"D:\OneDrive".to_owned())]);
    }

    #[test]
    fn dropbox_folders_come_from_info_json() {
        let found = find_with(
            &[("LOCALAPPDATA", r"C:\Users\me\AppData\Local")],
            &[(
                r"C:\Users\me\AppData\Local\Dropbox\info.json",
                r#"{"personal": {"path": "C:\\Users\\me\\Dropbox", "host": 1},
                    "business": {"path": "C:\\Users\\me\\Dropbox (Contoso)"}}"#,
            )],
        );
        assert_eq!(
            found,
            vec![
                ("dropbox", r"C:\Users\me\Dropbox".to_owned()),
                (
                    "dropbox_business",
                    r"C:\Users\me\Dropbox (Contoso)".to_owned()
                ),
            ]
        );
    }

    #[test]
    fn broken_dropbox_info_is_ignored() {
        let found = find_with(
            &[("APPDATA", r"C:\Users\me\AppData\Roaming")],
            &[(r"C:\Users\me\AppData\Roaming\Dropbox\info.json", "not json")],
        );
        assert!(found.is_empty());
    }

    #[test]
    fn conflict_copy_names() {
        let is_copy = conflict_copy_matcher("profiles-sync.json");
        // OneDrive appends the PC name.
        assert!(is_copy("profiles-sync-DESKTOP-ABC.json"));
        assert!(is_copy("profiles-sync-DESKTOP-ABC-2.json"));
        // Dropbox, in English and Japanese.
        assert!(is_copy(
            "profiles-sync (DESKTOP-ABC's conflicted copy 2026-10-31).json"
        ));
        assert!(is_copy("profiles-sync (me の競合コピー 2026-10-31).json"));
        assert!(is_copy("Profiles-Sync-PC.JSON"));

        assert!(!is_copy("profiles-sync.json"));
        assert!(!is_copy("profiles-syncs.json"));
        assert!(!is_copy("profiles-sync-.json"));
        // Backups made by hand are not conflict copies (they would be deleted).
        assert!(!is_copy("profiles-sync - コピー.json"));
        assert!(!is_copy("profiles-sync (1).json"));
        assert!(!is_copy("profiles-sync-backup.json"));
        assert!(!is_copy("profiles-sync-2.json"));
        assert!(!is_copy("profiles-sync (old).json"));
        assert!(!is_copy("profiles-sync-PC.json.tmp"));
        assert!(!is_copy("~profiles-sync.json.1-0.tmp"));
        assert!(!is_copy("other.json"));
    }

    #[test]
    fn write_atomic_replaces_and_leaves_no_temporary_file() {
        let dir = std::env::temp_dir().join(format!("musql_sync_atomic_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(SYNC_FILE);

        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"second").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from(SYNC_FILE)]);

        std::fs::write(dir.join("profiles-sync-PC.json"), "{}").unwrap();
        std::fs::write(dir.join("unrelated.json"), "{}").unwrap();
        assert_eq!(
            conflict_copies(&path),
            vec![dir.join("profiles-sync-PC.json")]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
