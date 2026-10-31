//! Captures the manual's screenshots (`just screenshots`). Built only with the
//! `screenshots` feature, and active only when `MUSQL_SCREENSHOTS_OUT` names the output
//! folder: the app then runs `SHOTS` against its own windows and exits.
//!
//! Each shot drives the pages with JavaScript (`WebviewWindow::eval`; the page reports back
//! through the `screenshots:done` event) and captures the webview with WebView2's
//! `CapturePreview`. The capture holds the page only, not the window frame.
//!
//! The pictures depend on the machine, so the setup checks what it can and fails instead of
//! taking a different picture: the page's `devicePixelRatio` must be `SCALE` (the build passes
//! `--force-device-scale-factor` to WebView2, `tauri.screenshots.conf.json`), and each image
//! must come out at the window's size times `SCALE`. The data comes from the fixtures in
//! `docs/manual/fixtures/` (profiles, and a MySQL with the demo databases).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use tauri::plugin::TauriPlugin;
use tauri::{AppHandle, Listener, Manager, PhysicalSize, Runtime, WebviewWindow};

use crate::{WIN_MAIN, WIN_QUERY, WIN_SETTINGS};

/// The folder to write the images to. The app captures only when this is set.
const OUT_ENV: &str = "MUSQL_SCREENSHOTS_OUT";

/// The display scale the manual's images are taken at (Windows' 150%).
const SCALE: f64 = 1.5;

/// How long one step may take (a cold MySQL connection included).
const STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// The fixture profile the query shots connect with, and its password (`fixtures/README.md`).
/// `SHOTS` opens it by this id.
const DEMO_PROFILE: &str = "prof-local-docker";
const DEMO_PASSWORD: &str = "demo1234";

fn out_dir() -> Option<PathBuf> {
    std::env::var_os(OUT_ENV).map(PathBuf::from)
}

pub fn active() -> bool {
    out_dir().is_some()
}

/// Loaded into every page before its own scripts. Starts each run from the same state
/// (Japanese, dark theme, no remembered tabs or drafts) and gives the steps their helpers.
/// The windows share one localStorage, so it is reset once per run (`RUN_ID`), not by
/// each page that loads.
const PRELUDE: &str = r#"(() => {
  try {
    if (localStorage.getItem("musql:screenshots-run") !== RUN_ID) {
      for (const key of Object.keys(localStorage)) {
        if (key.startsWith("musql:")) localStorage.removeItem(key);
      }
      localStorage.setItem("musql:screenshots-run", RUN_ID);
      localStorage.setItem("musql:lang", "ja");
      localStorage.setItem("musql:theme", "dark");
    }
  } catch (_) {}
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const shot = {
    async waitUntil(test, what, timeout = 30000) {
      const end = Date.now() + timeout;
      for (;;) {
        const value = test();
        if (value) return value;
        if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
        await sleep(100);
      }
    },
    waitFor(selector, timeout) {
      return shot.waitUntil(() => {
        const el = document.querySelector(selector);
        return el && el.getClientRects().length > 0 ? el : null;
      }, selector, timeout);
    },
    // The button in `scope` whose text contains `text` (buttons carry an icon and a label).
    button(scope, text) {
      const found = [...document.querySelectorAll(`${scope} button`)]
        .find((b) => b.textContent.includes(text));
      if (!found) throw new Error(`no button "${text}" in ${scope}`);
      return found;
    },
    // Fonts loaded and two frames painted: what the capture right after will show.
    async settle() {
      await document.fonts.ready;
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      await sleep(300);
    },
    run(token, body, settle) {
      const done = (error) =>
        window.__TAURI__.event.emit("screenshots:done", { token, error });
      Promise.resolve()
        .then(body)
        .then(() => settle && shot.settle())
        .then(() => done(null), (e) => done(String((e && e.message) || e)));
    },
  };
  window.__shot = shot;
})();"#;

pub fn plugin<R: Runtime>() -> TauriPlugin<R> {
    let run_id = format!("\"{}\"", std::process::id());
    tauri::plugin::Builder::new("musql-screenshots")
        .js_init_script(PRELUDE.replace("RUN_ID", &run_id))
        .build()
}

/// One picture: the steps that set the windows up, then the window to capture.
struct Shot {
    file: &'static str,
    steps: &'static [(&'static str, &'static str)],
    capture: &'static str,
}

/// The shots, in order. Each starts from where the previous one left the windows, so the
/// main window comes first (opening a connection hides it).
const SHOTS: &[Shot] = &[
    Shot {
        file: "profile-list-colored.png",
        steps: &[(WIN_MAIN, r#"await __shot.waitFor(".tree-item-tags");"#)],
        capture: WIN_MAIN,
    },
    Shot {
        file: "sync-settings.png",
        steps: &[(
            WIN_MAIN,
            r#"openSyncModal();
            await __shot.waitFor(".sync-folder-item");"#,
        )],
        capture: WIN_MAIN,
    },
    Shot {
        file: "profile-edit.png",
        steps: &[
            (
                WIN_MAIN,
                r#"document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
                openSettings("prof-ec-prod");"#,
            ),
            (
                WIN_SETTINGS,
                r#"await __shot.waitUntil(
                  () => document.getElementById("mysql-host").value === "db01.ec-prod.example.internal",
                  "the profile in the form");"#,
            ),
        ],
        capture: WIN_SETTINGS,
    },
    Shot {
        file: "query-sql.png",
        steps: &[
            (WIN_MAIN, r#"openQuery("prof-local-docker");"#),
            (
                WIN_QUERY,
                r#"await __shot.waitFor(".table-list-item");
                // Into the SQL tab the connection opens (CodeMirror 5 hangs the instance on
                // its element), so the tab bar shows one tab.
                const editor = (await __shot.waitFor(".tab-pane.active .CodeMirror")).CodeMirror;
                editor.setValue("SELECT id, name, email, prefecture, registered_at\nFROM customers\nORDER BY registered_at DESC\nLIMIT 20;");
                __shot.button(".tab-pane.active .actions", t("run_all")).click();
                await __shot.waitFor(".tab-pane.active .sql-result-area table");"#,
            ),
        ],
        capture: WIN_QUERY,
    },
];

/// The sync folders the sync settings list while capturing, in place of this machine's
/// (which would show the user's own paths).
pub fn sync_folders() -> Vec<crate::sync_folder::SyncFolder> {
    [
        ("onedrive", r"C:\Users\you\OneDrive"),
        ("dropbox", r"C:\Users\you\Dropbox"),
    ]
    .into_iter()
    .map(|(service, folder)| crate::sync_folder::SyncFolder {
        service,
        folder: folder.to_owned(),
        file: format!(r"{folder}\muSQL\profiles-sync.json"),
        exists: false,
    })
    .collect()
}

#[derive(serde::Deserialize)]
struct Done {
    token: u64,
    error: Option<String>,
}

/// Starts the capture when `MUSQL_SCREENSHOTS_OUT` is set. Runs on its own thread (each
/// step waits on the page) and exits the app when done: 0 when every image was written.
pub fn start(app: &AppHandle) {
    let Some(out) = out_dir() else {
        return;
    };
    let (tx, rx) = mpsc::channel();
    app.listen_any("screenshots:done", move |event| {
        if let Ok(done) = serde_json::from_str::<Done>(event.payload()) {
            let _ = tx.send(done);
        }
    });
    let app = app.clone();
    std::thread::spawn(move || {
        // The fixture profile's id may also be in the dev build's fixtures, with a password
        // saved there (the keyring is shared): put back what was there.
        let previous = crate::get_password(DEMO_PROFILE);
        let result = Runner {
            app: &app,
            done: rx,
            token: 0,
        }
        .run(&out);
        let _ = crate::set_password(DEMO_PROFILE, &previous);
        let code = match result {
            Ok(()) => {
                println!(
                    "screenshots: wrote {} images to {}",
                    SHOTS.len(),
                    out.display()
                );
                0
            }
            Err(e) => {
                eprintln!("screenshots: {e}");
                1
            }
        };
        app.exit(code);
    });
}

struct Runner<'a> {
    app: &'a AppHandle,
    done: Receiver<Done>,
    token: u64,
}

impl Runner<'_> {
    fn run(&mut self, out: &Path) -> Result<(), String> {
        if !SHOTS
            .iter()
            .flat_map(|shot| shot.steps)
            .any(|(_, script)| script.contains(DEMO_PROFILE))
        {
            return Err(format!(
                "no shot opens {DEMO_PROFILE}, whose password is set"
            ));
        }
        std::fs::create_dir_all(out)
            .map_err(|e| format!("cannot create {}: {e}", out.display()))?;
        crate::set_password(DEMO_PROFILE, DEMO_PASSWORD)?;
        // The pages may still be loading: a script sent before the page commits is lost with
        // the old document, so ask again until each page answers.
        for label in [WIN_MAIN, WIN_SETTINGS, WIN_QUERY] {
            self.wait_ready(label)?;
        }
        let check = format!(
            r#"if (window.devicePixelRatio !== {SCALE}) throw new Error(
              `devicePixelRatio is ${{window.devicePixelRatio}}, not {SCALE} (check additionalBrowserArgs)`);"#
        );
        self.step(WIN_MAIN, &check, false, STEP_TIMEOUT)?;
        for shot in SHOTS {
            for (label, script) in shot.steps {
                self.step(label, script, false, STEP_TIMEOUT)
                    .map_err(|e| format!("{}: {label}: {e}", shot.file))?;
            }
            self.capture(shot.capture, &out.join(shot.file))
                .map_err(|e| format!("{}: {e}", shot.file))?;
            println!("screenshots: {}", shot.file);
        }
        Ok(())
    }

    fn window(&self, label: &str) -> Result<WebviewWindow, String> {
        self.app
            .get_webview_window(label)
            .ok_or_else(|| format!("no window {label}"))
    }

    fn wait_ready(&mut self, label: &str) -> Result<(), String> {
        for _ in 0..30 {
            if self.step(label, "", false, Duration::from_secs(2)).is_ok() {
                return Ok(());
            }
        }
        Err(format!("the {label} page did not load"))
    }

    /// Runs `script` (the body of an async function) in the window's page and waits for it,
    /// and with `settle` for the page to paint the result.
    fn step(
        &mut self,
        label: &str,
        script: &str,
        settle: bool,
        timeout: Duration,
    ) -> Result<(), String> {
        self.token += 1;
        let token = self.token;
        // A page that is still loading has no `__shot` yet; retry until it does, but not past
        // the timeout (a later step sends its own script).
        let tries = timeout.as_millis() / 100;
        let wrapped = format!(
            "(function go(n) {{ if (!window.__shot || !window.__TAURI__) {{ if (n > 0) setTimeout(() => go(n - 1), 100); return; }} \
             __shot.run({token}, async () => {{ {script} }}, {settle}); }})({tries});"
        );
        self.window(label)?
            .eval(&wrapped)
            .map_err(|e| format!("eval failed: {e}"))?;
        // Answers to earlier steps that timed out may still arrive; skip them.
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.done.recv_timeout(left) {
                Ok(done) if done.token == token => return done.error.map_or(Ok(()), Err),
                Ok(_) => {}
                Err(_) => return Err("timed out".to_owned()),
            }
        }
    }

    /// Sizes the window as in `tauri.conf.json` at `SCALE` and writes its page to `path` as
    /// PNG.
    fn capture(&mut self, label: &str, path: &Path) -> Result<(), String> {
        let win = self.window(label)?;
        let config = self
            .app
            .config()
            .app
            .windows
            .iter()
            .find(|w| w.label == label)
            .ok_or_else(|| format!("no window {label} in the config"))?;
        // Physical pixels: the page's CSS size is the config's whatever the OS scale is.
        let size = PhysicalSize::new(
            (config.width * SCALE).round() as u32,
            (config.height * SCALE).round() as u32,
        );
        // A resizable window is kept within the screen (Windows' default maximum tracking
        // size), and the CI runner's screen is 1024x768. tao lifts that limit only when a
        // maximum size is set.
        win.set_max_size(Some(size))
            .and_then(|()| win.set_size(size))
            .map_err(|e| format!("resize failed: {e}"))?;
        let laid_out = format!(
            "await __shot.waitUntil(() => innerWidth === {} && innerHeight === {}, 'the window size');",
            config.width, config.height
        );
        self.step(label, &laid_out, true, STEP_TIMEOUT)?;

        let (tx, rx) = mpsc::channel();
        win.with_webview(move |webview| {
            if let Err(e) = capture_preview(&webview.controller(), tx.clone()) {
                let _ = tx.send(Err(format!("CapturePreview failed: {e}")));
            }
        })
        .map_err(|e| format!("with_webview failed: {e}"))?;
        let png = rx
            .recv_timeout(STEP_TIMEOUT)
            .map_err(|_| "capture timed out".to_owned())??;

        let actual = png_size(&png).ok_or("the capture is not a PNG")?;
        if actual != (size.width, size.height) {
            return Err(format!(
                "captured {}x{}, expected {}x{}",
                actual.0, actual.1, size.width, size.height
            ));
        }
        std::fs::write(path, png).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

/// Starts WebView2's capture into a memory stream. Runs on the main thread; the completion
/// handler (also on the main thread) sends the PNG bytes.
fn capture_preview(
    controller: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
    tx: Sender<Result<Vec<u8>, String>>,
) -> windows::core::Result<()> {
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use windows::Win32::UI::Shell::SHCreateMemStream;

    unsafe {
        let core = controller.CoreWebView2()?;
        let stream = SHCreateMemStream(None)
            .ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY))?;
        let handler_stream = stream.clone();
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
            let _ = tx.send(
                result
                    .and_then(|()| read_stream(&handler_stream))
                    .map_err(|e| format!("capture failed: {e}")),
            );
            Ok(())
        }));
        core.CapturePreview(
            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
            &stream,
            &handler,
        )
    }
}

fn read_stream(stream: &windows::Win32::System::Com::IStream) -> windows::core::Result<Vec<u8>> {
    use windows::Win32::System::Com::{STATFLAG_NONAME, STATSTG, STREAM_SEEK_SET};

    unsafe {
        let mut stat = STATSTG::default();
        stream.Stat(&mut stat, STATFLAG_NONAME)?;
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        let mut data = vec![0u8; stat.cbSize as usize];
        let mut read = 0u32;
        stream
            .Read(data.as_mut_ptr().cast(), data.len() as u32, Some(&mut read))
            .ok()?;
        data.truncate(read as usize);
        Ok(data)
    }
}

/// The width and height in a PNG's IHDR chunk.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let read = |at: usize| {
        png.get(at..at + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    Some((read(16)?, read(20)?))
}
