//! The main window opens on the virtual desktop it was last on (#126).
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

use std::path::PathBuf;
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

/// Whether this process started while no other muSQL was running. Decided once, in
/// `restore`.
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

/// At startup: moves `window` (main) back to the desktop it was last on, if this is the
/// only muSQL running. Call it once, from `setup`.
pub(crate) fn restore(app: &AppHandle, window: &WebviewWindow) {
    let first = *FIRST_INSTANCE.get_or_init(|| claim_first_instance(&app.config().identifier));
    if !first {
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

/// At quit: remembers the desktop this instance is on, and stops counting as running.
///
/// `windows` are tried in order and the first one that is on a desktop decides: main,
/// then query (main is hidden while the query window is open, and a hidden window is on
/// no desktop). If none is, what was remembered stays.
///
/// Every instance writes, so the desktop remembered is that of the muSQL closed last.
pub(crate) fn on_quit(app: &AppHandle, windows: &[&str]) {
    let id = windows
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .find_map(|w| desktop_id(&w));
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
