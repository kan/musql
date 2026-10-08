//! Desktop notifications that can be clicked (#113).
//!
//! A long query finishes while the window is in the background; the toast says so, and
//! **clicking it brings the query window forward on the tab that ran the query**.
//!
//! ## Why not the notification plugin
//!
//! `tauri-plugin-notification` hands the toast to `notify_rust` and has no way to hear
//! a click on desktop (`onAction` is mobile-only). The WebView's own `Notification` does
//! not deliver `onclick` under WebView2 either. So the toast is built here, with
//! `tauri-winrt-notification` (already in the tree through the plugin), whose
//! `on_activated` fires in this process.
//!
//! ## The click only comes back while the banner is on screen
//!
//! `on_activated` is an in-process event. Once a toast has moved to the notification
//! centre, Windows no longer raises it; a click there needs a COM activator or a
//! protocol launch, both of which start a *new* process. muSQL allows several instances
//! on purpose (`.claude/rules/rust.md`), so a new process would have no way to reach the
//! instance that owns the tab. In-process activation avoids the question: the instance
//! that showed the toast is the one that hears the click. Clicks from the notification
//! centre are out of scope: clicking a toast there does nothing (checked on a dev build;
//! in particular it does not launch the shortcut's target).
//!
//! ## AUMID and the Start Menu shortcut
//!
//! An unpackaged desktop app only gets proper toasts (a banner, and clicks) when a Start
//! Menu shortcut carries its AppUserModelID. Without one the toast goes straight to the
//! notification centre and the click never arrives. So the shortcut is prepared here,
//! lazily, on the first toast: the installer's shortcut gets the AUMID added, and a dev
//! build creates one of its own.
//!
//! The AUMID is the app identifier, and **the shortcut is chosen from the identifier
//! too**, never from the build profile: two shortcuts with one AUMID would leave Windows
//! to pick either for the toast's name and icon.
//!
//! The *process* AUMID is not set (`SetCurrentProcessExplicitAppUserModelID` is never
//! called). It is not needed for the click, and setting it would change how the taskbar
//! groups the windows.
//!
//! ## The Store build
//!
//! A packaged (MSIX) app takes its AUMID from the package and has no shortcut to edit.
//! `toast_notify` answers `false` there, and the UI falls back to the plugin (a toast
//! without a click, as before).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter, EventTarget, Manager};
use tauri_winrt_notification::Toast;
use windows::core::{Interface, GUID, HSTRING, PWSTR};
use windows::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, E_OUTOFMEMORY, PROPERTYKEY};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemAlloc, IPersistFile, CLSCTX_INPROC_SERVER,
    COINIT_APARTMENTTHREADED, STGM_READWRITE,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

/// `PKEY_AppUserModel_ID`. Written out because the crate's PKEY constants sit behind
/// another feature.
const PKEY_APPUSERMODEL_ID: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

/// Event sent to the window that owns the tab when its toast is clicked.
const ACTIVATED_EVENT: &str = "toast:activated";

/// The name NSIS gives the installed shortcut: `productName` in tauri.conf.json (a test
/// keeps the two in step).
const PRODUCT_NAME: &str = "muSQL";

/// Name of the Start Menu shortcut that carries `aumid`.
///
/// The installed build uses the installer's own shortcut, so the AUMID is added to the
/// existing entry instead of creating a second "muSQL". The dev identifier has no
/// shortcut and gets its own; that one exists only to register the AUMID.
pub(crate) fn link_name(aumid: &str) -> String {
    if crate::is_debug_identifier(aumid) {
        format!("{PRODUCT_NAME} (dev).lnk")
    } else {
        format!("{PRODUCT_NAME}.lnk")
    }
}

/// `%APPDATA%\Microsoft\Windows\Start Menu\Programs\<name>`.
///
/// Only the per-user Start Menu is looked at, which is where NSIS puts the shortcut.
/// An MSI install puts it machine-wide, where it cannot be edited without elevation;
/// that case ends up with a second, per-user "muSQL" entry. Accepted.
fn link_path(aumid: &str) -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(
        PathBuf::from(appdata)
            .join(r"Microsoft\Windows\Start Menu\Programs")
            .join(link_name(aumid)),
    )
}

/// Whether this process runs with package identity (the Store / MSIX build).
///
/// Asked at run time, not through a cargo feature: the Store EXE is also published on
/// its own, and run outside a package it is an ordinary desktop app.
fn is_packaged() -> bool {
    let mut len = 0u32;
    // With no buffer this returns ERROR_INSUFFICIENT_BUFFER for a packaged process and
    // APPMODEL_ERROR_NO_PACKAGE otherwise.
    unsafe { GetCurrentPackageFullName(&mut len, None) != APPMODEL_ERROR_NO_PACKAGE }
}

/// A `VT_LPWSTR` PROPVARIANT. The string is allocated with `CoTaskMemAlloc`, which is
/// what `PropVariantClear` frees; the crate's `Drop` for `PROPVARIANT` calls that, so
/// the value returned here cleans up after itself.
///
/// # Safety
/// Must be called on a thread where COM is initialised.
unsafe fn lpwstr_propvariant(s: &str) -> windows::core::Result<PROPVARIANT> {
    let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
    let mem = CoTaskMemAlloc(wide.len() * 2) as *mut u16;
    if mem.is_null() {
        return Err(windows::core::Error::from(E_OUTOFMEMORY));
    }
    std::ptr::copy_nonoverlapping(wide.as_ptr(), mem, wide.len());
    Ok(PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 {
                    pwszVal: PWSTR(mem),
                },
            }),
        },
    })
}

/// The AUMID already stored in the shortcut, if it is in the form written here.
unsafe fn current_aumid(store: &IPropertyStore) -> Option<String> {
    let pv = store.GetValue(&PKEY_APPUSERMODEL_ID).ok()?;
    let inner = &*pv.Anonymous.Anonymous;
    if inner.vt != VT_LPWSTR {
        return None;
    }
    let p = inner.Anonymous.pwszVal;
    (!p.is_null()).then(|| p.to_string().ok()).flatten()
}

/// Makes sure the Start Menu shortcut carries `aumid`. The two kinds of build are
/// treated differently on purpose.
///
/// **Installed build: amend only, never create.** The installer's shortcut is loaded
/// and gets the AUMID added; its target and working directory are the installer's to
/// decide. When there is no shortcut this fails, and the caller falls back to a plain
/// toast. Creating one would plant a "muSQL" entry pointing at wherever this exe happens
/// to be (the Store EXE run on its own from Downloads, say) with nothing to remove it,
/// or a second entry next to a machine-wide one.
///
/// **Dev identifier: ours to own.** The shortcut exists only to register the AUMID, so
/// it is created when missing and pointed at `exe` every time (the path of a dev binary
/// is not stable, and `just verify-toast` runs from a different exe than `just dev`).
///
/// # Safety
/// Must be called on a thread where COM is initialised (`init_com`).
pub(crate) unsafe fn ensure_shortcut(aumid: &str, exe: &Path) -> Result<(), String> {
    let path = link_path(aumid).ok_or("APPDATA is not set")?;
    let dev = crate::is_debug_identifier(aumid);
    let exists = path.is_file();
    if !exists && !dev {
        return Err(format!("no Start Menu shortcut at {}", path.display()));
    }

    let com = |e: windows::core::Error| format!("{e:?}");
    let wide_path = HSTRING::from(path.to_string_lossy().as_ref());
    let link: IShellLinkW =
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(com)?;
    let file: IPersistFile = link.cast().map_err(com)?;
    let store: IPropertyStore = link.cast().map_err(com)?;

    if exists {
        file.Load(&wide_path, STGM_READWRITE).map_err(com)?;
        if !dev && current_aumid(&store).as_deref() == Some(aumid) {
            return Ok(());
        }
    }
    if dev {
        let exe = HSTRING::from(exe.to_string_lossy().as_ref());
        link.SetPath(&exe).map_err(com)?;
        link.SetIconLocation(&exe, 0).map_err(com)?;
    }

    let pv = lpwstr_propvariant(aumid).map_err(com)?;
    store.SetValue(&PKEY_APPUSERMODEL_ID, &pv).map_err(com)?;
    store.Commit().map_err(com)?;
    file.Save(&wide_path, true).map_err(com)?;
    Ok(())
}

/// Shows one toast and calls `on_click` if it is clicked while on screen.
///
/// The `Toast` value is not kept: registering the handler makes the COM side hold what
/// it needs, so the click still arrives after this returns. **It does not survive the
/// thread, though.** Observed, not derived: when the thread that called this ends, the
/// handler is released at once and a click does nothing. Call it from a thread that
/// stays alive for as long as a click should work.
///
/// `on_click` runs on a WinRT thread-pool thread, not on the calling thread.
///
/// # Safety
/// Must be called on a thread where COM is initialised (`init_com`).
pub(crate) unsafe fn show(
    aumid: &str,
    title: &str,
    body: &str,
    on_click: impl Fn() + Send + 'static,
) -> Result<(), String> {
    Toast::new(aumid)
        .title(title)
        .text1(body)
        .on_activated(move |_action| {
            on_click();
            Ok(())
        })
        .show()
        .map_err(|e| format!("{e:?}"))
}

/// Initialises COM on the current thread the way `ensure_shortcut` and `show` need it.
pub(crate) fn init_com() -> bool {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok()
}

/// One toast to show, and where its click leads.
struct Job {
    app: AppHandle,
    /// The window to bring forward.
    window: String,
    /// What the UI wants back when the toast is clicked. Opaque here: Rust knows nothing
    /// about tabs or sessions.
    token: String,
    title: String,
    body: String,
    /// Where the outcome goes. The command waits for it, so that a toast that could not
    /// be shown makes the UI fall back instead of vanishing.
    reply: Sender<Result<(), String>>,
}

/// The sender of the toast thread, started on the first toast.
///
/// A dedicated thread because of COM: commands run on tokio workers, which are not
/// initialised.
///
/// **The thread must never end, and toasts must not be shown from short-lived threads.**
/// When the thread that showed a toast ends, its click handler is released: the toast
/// still appears, and clicking it silently does nothing (see `show`). No unit test can
/// catch that; `just verify-toast` did, once.
fn worker() -> &'static Sender<Job> {
    static WORKER: OnceLock<Sender<Job>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        let spawned = std::thread::Builder::new()
            .name("toast".into())
            .spawn(move || {
                if !init_com() {
                    log::warn!("toast: CoInitializeEx failed; click-through toasts disabled");
                    return;
                }
                // Checked until it succeeds once, then left alone: reading the shortcut
                // on every toast buys nothing.
                let mut shortcut_ready = false;
                while let Ok(job) = rx.recv() {
                    let reply = job.reply.clone();
                    let _ = reply.send(show_job(job, &mut shortcut_ready));
                }
            });
        // If the thread could not start, or COM failed above, `rx` is dropped: `send`
        // fails, or the job is dropped with its reply sender. `toast_notify` reports
        // both as an error.
        if let Err(e) = spawned {
            log::warn!("toast: could not start the toast thread: {e}");
        }
        tx
    })
}

/// Shows one job's toast. Runs on the toast thread.
fn show_job(job: Job, shortcut_ready: &mut bool) -> Result<(), String> {
    let aumid = job.app.config().identifier.clone();
    if !*shortcut_ready {
        // Without the shortcut the toast gets no banner and no click, so there is no
        // point showing it: fail, and let the UI use the plain toast instead.
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        unsafe { ensure_shortcut(&aumid, &exe) }
            .map_err(|e| format!("shortcut setup failed: {e}"))?;
        *shortcut_ready = true;
    }
    let (title, body) = (job.title.clone(), job.body.clone());
    let on_click = move || {
        // On a WinRT thread here, so the window work goes to the main thread.
        let app = job.app.clone();
        let window = job.window.clone();
        let token = job.token.clone();
        let _ = app.clone().run_on_main_thread(move || {
            let Some(win) = app.get_webview_window(&window) else {
                return;
            };
            // Hidden means closed here (the show/hide window pattern): the session the
            // toast belonged to is over, and showing the window would reveal an empty,
            // reset one. A minimised window still counts as visible.
            if !win.is_visible().unwrap_or(false) {
                return;
            }
            let _ = win.unminimize();
            let _ = win.set_focus();
            let _ = app.emit_to(EventTarget::webview_window(&window), ACTIVATED_EVENT, token);
        });
    };
    unsafe { show(&aumid, &title, &body, on_click) }
}

/// How long `toast_notify` waits for the toast thread. Showing takes tens of
/// milliseconds; this only bounds a thread that is stuck.
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// Shows a toast for the calling window. Clicking it focuses that window and hands
/// `token` back to it (`toast:activated`).
///
/// Whether to notify at all is the UI's decision (the setting lives there).
///
/// - `Ok(true)`: the toast was shown.
/// - `Ok(false)`: this build cannot show a clickable toast (the Store build). Expected;
///   the caller falls back to the plugin and need not ask again.
/// - `Err`: it could not be shown this time (no shortcut to carry the AUMID, the toast
///   thread is gone, Windows refused). Logged here; the caller falls back.
#[tauri::command]
pub async fn toast_notify(
    window: tauri::WebviewWindow,
    token: String,
    title: String,
    body: String,
) -> Result<bool, String> {
    if is_packaged() {
        return Ok(false);
    }
    let (reply, outcome) = channel();
    let sent = worker().send(Job {
        app: window.app_handle().clone(),
        window: window.label().to_owned(),
        token,
        title,
        body,
        reply,
    });
    let result = match sent {
        Err(_) => Err("the toast thread is not running".to_owned()),
        // The wait blocks, so it goes to the blocking pool, not an async worker.
        Ok(()) => tauri::async_runtime::spawn_blocking(move || {
            outcome
                .recv_timeout(REPLY_TIMEOUT)
                .unwrap_or_else(|_| Err("the toast thread did not answer".to_owned()))
        })
        .await
        .unwrap_or_else(|e| Err(format!("Task error: {e}"))),
    };
    result
        .map(|()| true)
        // Throttled: the same failure repeats for every long query.
        .inspect_err(|e| {
            crate::app_log::warn_throttled(crate::app_log::LONG, &format!("toast: {e}"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_name_follows_the_identifier() {
        assert_eq!(link_name("jp.co.communitylinks.musql"), "muSQL.lnk");
        assert_eq!(
            link_name("jp.co.communitylinks.musql.debug"),
            "muSQL (dev).lnk"
        );
    }

    #[test]
    fn product_name_matches_the_installer() {
        // NSIS names the shortcut after `productName`. If that changes, the installed
        // build would stop finding its shortcut and create a second one.
        let conf = include_str!("../tauri.conf.json");
        assert!(
            conf.contains(&format!("\"productName\": \"{PRODUCT_NAME}\"")),
            "PRODUCT_NAME in toast.rs and productName in tauri.conf.json differ"
        );
    }

    #[test]
    fn link_path_is_under_the_user_start_menu() {
        let path = link_path("jp.co.communitylinks.musql.debug").expect("APPDATA is set");
        assert!(path.ends_with(r"Microsoft\Windows\Start Menu\Programs\muSQL (dev).lnk"));
    }

    #[test]
    fn a_test_process_has_no_package_identity() {
        assert!(!is_packaged());
    }
}
