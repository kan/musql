//! The main window opens on the virtual desktop it was last on (#126), when that is turned
//! on (#144).
//!
//! ## Off unless turned on
//!
//! A window that jumps to another desktop at startup is a surprise to someone who started
//! muSQL on the desktop they are working on, so this is off by default and turned on from
//! the main window's View menu. The choice is the AppConfig file `remember_desktop`
//! (`1` when on), read by Rust at startup, before any page could. It is per machine and
//! never synced. Off, nothing is recorded or restored. Switching it either way forgets the
//! desktop remembered, so that turning it on never jumps to one recorded before (by an
//! earlier version, which always recorded, or before it was last turned off).
//!
//! Size and position are restored by `tauri-plugin-window-state`, which knows nothing
//! about virtual desktops: a window opens on whichever desktop is showing. This remembers
//! the desktop's GUID through `IVirtualDesktopManager` (the public COM API; pike does the
//! same) and moves the window back at startup.
//!
//! ## Only the first instance
//!
//! muSQL may run several times at once (one connection per instance, by design). If every
//! instance jumped to the remembered desktop, a second one could not be opened anywhere
//! else. So only an instance that starts while no other is running is moved. Later
//! instances open on the desktop that is showing, as before. "Is another one running" is
//! a named mutex: no messages between instances, and no `tauri-plugin-single-instance`.
//!
//! Every instance records its desktop when it quits, so what is remembered is where the
//! muSQL that was closed last had been.
//!
//! ## Only the main window
//!
//! The query and settings windows are opened from main, by a click. They stay where the
//! click was.
//!
//! ## A desktop that is gone
//!
//! The public API can only move a window to a desktop that exists. If the remembered one
//! was removed, the move fails and the window stays on the desktop that is showing.
//! Recreating desktops needs the private `IVirtualDesktopManagerInternal`, which breaks
//! with Windows updates; not used.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewWindow};
use windows::core::{GUID, HSTRING};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::{IVirtualDesktopManager, VirtualDesktopManager};

/// What a window that is on no desktop yet (minimised, not shown) reports.
const NULL_GUID: GUID = GUID::from_u128(0);

/// How long after `setup` the move is tried again.
const RETRY_AFTER: Duration = Duration::from_millis(500);

/// Whether this process started while no other muSQL was running. Decided once, by
/// `claim` (or `restore`, whichever runs first).
static FIRST_INSTANCE: OnceLock<bool> = OnceLock::new();

/// The mutex that says "a muSQL is running", as a raw handle; 0 when not held.
static INSTANCE_MUTEX: AtomicIsize = AtomicIsize::new(0);

/// Initialises COM on this thread for as long as it lives.
///
/// Every successful `CoInitializeEx` needs its `CoUninitialize`, including the one that
/// reports "already initialised" (`S_FALSE`, which is `Ok`: the main thread, where
/// WebView2 got there first). Only a failure (`RPC_E_CHANGED_MODE`, a thread in the other
/// threading model) took no reference and must not be undone.
struct ComScope(bool);

impl ComScope {
    fn enter() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

/// Declare a `ComScope` before calling this, so that the manager is dropped first.
fn manager() -> Option<IVirtualDesktopManager> {
    unsafe { CoCreateInstance(&VirtualDesktopManager, None, CLSCTX_ALL) }
        .inspect_err(|e| log::warn!("[vdesk] CoCreateInstance failed: {e}"))
        .ok()
}

fn hwnd(window: &WebviewWindow) -> Option<HWND> {
    window
        .hwnd()
        .inspect_err(|e| log::warn!("[vdesk] hwnd() failed: {e}"))
        .ok()
        .map(|h| HWND(h.0 as isize as *mut _))
}

/// The desktop a window is on, or `None` while it is on none (hidden, minimised, not
/// yet known to the shell). That is an ordinary answer, so a failure is not logged.
fn desktop_id(window: &WebviewWindow) -> Option<String> {
    let _com = ComScope::enter();
    let id = unsafe { manager()?.GetWindowDesktopId(hwnd(window)?) }.ok()?;
    (id != NULL_GUID).then(|| format!("{id:?}"))
}

/// Moves a window to a desktop. Fails when that desktop no longer exists.
fn move_to(window: &WebviewWindow, id: &str) -> bool {
    let _com = ComScope::enter();
    let Ok(guid) = GUID::try_from(id) else {
        log::warn!("[vdesk] not a guid: {id}");
        return false;
    };
    let (Some(manager), Some(hwnd)) = (manager(), hwnd(window)) else {
        return false;
    };
    unsafe { manager.MoveWindowToDesktop(hwnd, &guid) }
        .inspect_err(|e| log::warn!("[vdesk] MoveWindowToDesktop({id}) failed: {e}"))
        .is_ok()
}

/// Takes the per-identifier mutex and says whether this process created it. The handle
/// is held until `on_quit` (or the end of the process). Dev and installed builds have
/// different identifiers and do not count as each other.
fn claim_first_instance(identifier: &str) -> bool {
    let name = HSTRING::from(format!("Local\\{identifier}.instance"));
    match unsafe { CreateMutexW(None, false, &name) } {
        Ok(handle) => {
            // Read before anything else can overwrite the thread's last error.
            let first = (unsafe { GetLastError() }) != ERROR_ALREADY_EXISTS;
            INSTANCE_MUTEX.store(handle.0 as isize, Ordering::SeqCst);
            first
        }
        Err(e) => {
            // Cannot tell: behave like a later instance, which is what muSQL did before.
            log::warn!("[vdesk] CreateMutexW failed: {e}");
            false
        }
    }
}

/// Where the desktop is remembered. A file of its own next to `sync_path`: it is
/// per-machine, and never synced.
fn store(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resolve("main_desktop", tauri::path::BaseDirectory::AppConfig)
        .ok()
}

/// Where the choice to remember the desktop is kept (#144).
fn setting(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resolve("remember_desktop", tauri::path::BaseDirectory::AppConfig)
        .ok()
}

/// Whether the desktop is remembered. Off when the setting was never made.
pub(crate) fn is_enabled(app: &AppHandle) -> bool {
    setting(app).is_some_and(|path| read_flag(&path))
}

/// Turns remembering the desktop on or off.
pub(crate) fn set_enabled(app: &AppHandle, on: bool) -> Result<(), String> {
    let setting = setting(app).ok_or("No place for the setting")?;
    let remembered = store(app).ok_or("No place for the remembered desktop")?;
    set_enabled_at(&setting, &remembered, on)
}

/// Either way, the desktop remembered is forgotten first: what is there was recorded
/// before this choice (by an earlier version, which always recorded, or before the
/// setting was last turned off), and turning on must not restore it if the session then
/// ends without recording a new one. First, so that a failure leaves the setting as it
/// was rather than off with the old desktop still there.
fn set_enabled_at(setting: &Path, remembered: &Path, on: bool) -> Result<(), String> {
    remove_if_present(remembered)?;
    write_flag(setting, on)
}

fn read_flag(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.trim() == "1")
}

fn write_flag(path: &Path, on: bool) -> Result<(), String> {
    if !on {
        return remove_if_present(path);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, "1").map_err(|e| e.to_string())
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

/// At startup, whatever the setting: takes the mutex that says a muSQL is running. An
/// instance that started with the setting off must hold it too, or one started after it
/// with the setting on would take itself for the only muSQL and move to its desktop.
/// Call it from `setup`, before `restore`; a second call does nothing.
pub(crate) fn claim(app: &AppHandle) -> bool {
    *FIRST_INSTANCE.get_or_init(|| claim_first_instance(&app.config().identifier))
}

/// Moves the window and switches to that desktop.
///
/// Moving alone does not switch desktops; focusing the window does. Without it the app
/// would have been started and nothing would appear in front of the user. Windows may
/// still refuse the focus to a process that was not started by the user (a start at
/// login, the restart after an update); the window is then on its desktop, waiting.
fn move_and_show(window: &WebviewWindow, id: &str) -> bool {
    let moved = move_to(window, id);
    if moved {
        let _ = window.set_focus();
    }
    moved
}

/// At startup: moves `window` (main) back to the desktop it was last on, if that is turned
/// on and this is the only muSQL running. Call it once, from `setup`, after `claim` (it
/// takes the mutex itself if `claim` was not called, so the order cannot lose it).
pub(crate) fn restore(app: &AppHandle, window: &WebviewWindow) {
    if !claim(app) || !is_enabled(app) {
        return;
    }
    let Some(id) = store(app).and_then(|p| std::fs::read_to_string(p).ok()) else {
        return;
    };
    let id = id.trim().to_owned();
    if id.is_empty() {
        return;
    }
    // Moving to the desktop it is already on does nothing, so there is no check first.
    move_and_show(window, &id);

    // Once more when the window is up. `setup` runs before the event loop, and the shell
    // may not have taken the window under its management yet, in which case the move
    // above fails (pike moves twice for the same reason). Skipped when the first one
    // worked, so that focus is not taken a second time.
    let window = window.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(RETRY_AFTER).await;
        let on_main = window.clone();
        let _ = window.run_on_main_thread(move || {
            if desktop_id(&on_main).is_some_and(|now| now != id) {
                move_and_show(&on_main, &id);
            }
        });
    });
}

/// At quit: remembers the desktop this instance is on (when that is turned on), and stops
/// counting as running.
///
/// `windows` are tried in order and the first one that is on a desktop decides: main,
/// then the connection windows (main is hidden while one is open, and a hidden window is
/// on no desktop). If none is, what was remembered stays.
///
/// Every instance writes, so the desktop remembered is that of the muSQL closed last.
pub(crate) fn on_quit(app: &AppHandle, windows: &[&str]) {
    let id = windows
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .find_map(|w| desktop_id(&w))
        .filter(|_| is_enabled(app));
    if let (Some(id), Some(path)) = (id, store(app)) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, id) {
            log::warn!("[vdesk] could not remember the desktop: {e}");
        }
    }
    // Now, not when the process ends: quitting may wait several seconds for Docker, and
    // a muSQL started in that time would take itself for a second instance.
    let handle = INSTANCE_MUTEX.swap(0, Ordering::SeqCst);
    if handle != 0 {
        let _ = unsafe { CloseHandle(HANDLE(handle as *mut _)) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A directory of its own under the temp directory, removed when dropped (also when
    /// an assertion fails).
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("musql-vdesk-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // Off unless turned on (#144): a setting never made, or turned off, reads as off.
    #[test]
    fn the_setting_is_off_until_turned_on() {
        let dir = TempDir::new("flag");
        let path = dir.0.join("remember_desktop");
        assert!(!read_flag(&path));

        write_flag(&path, true).unwrap();
        assert!(read_flag(&path));
        write_flag(&path, false).unwrap();
        assert!(!read_flag(&path));
        assert!(!path.exists());
        // Off again, with nothing to remove.
        write_flag(&path, false).unwrap();

        std::fs::write(&path, "yes").unwrap();
        assert!(!read_flag(&path), "only 1 means on");
    }

    // A desktop recorded before the choice (by a version that always recorded) is not
    // restored after turning the setting on, nor kept after turning it off.
    #[test]
    fn switching_the_setting_forgets_the_desktop() {
        let dir = TempDir::new("switch");
        let setting = dir.0.join("remember_desktop");
        let remembered = dir.0.join("main_desktop");
        std::fs::create_dir_all(&dir.0).unwrap();

        for on in [true, false] {
            std::fs::write(&remembered, "{00000000-0000-0000-0000-000000000001}").unwrap();
            set_enabled_at(&setting, &remembered, on).unwrap();
            assert_eq!(read_flag(&setting), on);
            assert!(
                !remembered.exists(),
                "turned {}",
                if on { "on" } else { "off" }
            );
        }
        // Nothing remembered yet is fine too.
        set_enabled_at(&setting, &remembered, true).unwrap();
    }
}
