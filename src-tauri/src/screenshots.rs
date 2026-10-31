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
    // The first shown element that matches (hidden modals hold matching elements too).
    waitFor(selector, timeout) {
      return shot.waitUntil(() => [...document.querySelectorAll(selector)]
        .find((el) => el.getClientRects().length > 0), selector, timeout);
    },
    // The shown button in `scope` whose text contains `text` (buttons carry an icon and a
    // label).
    button(scope, text) {
      const found = [...document.querySelectorAll(`${scope} button`)]
        .find((b) => b.textContent.includes(text) && b.getClientRects().length > 0);
      if (!found) throw new Error(`no button "${text}" in ${scope}`);
      return found;
    },
    // Answers the page's commands from `table` (a value, or a function of the arguments)
    // instead of the app, for what would show the capturing machine (its Docker
    // containers, its sync folders, whether it has 1Password or an AI key). Every page
    // sends its commands through its own top-level `safeInvoke`, so wrapping it is enough.
    mock(table) {
      const real = window.safeInvoke;
      window.safeInvoke = (command, args) => Object.hasOwn(table, command)
        ? Promise.resolve(typeof table[command] === "function" ? table[command](args) : table[command])
        : real(command, args);
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
    steps: &'static [Step],
    capture: &'static str,
}

enum Step {
    /// Runs the script (the body of an async function) in the window's page.
    Js(&'static str, &'static str),
    /// Replaces the profiles with a fixture file (the pages reload them in a later step).
    Profiles(&'static str),
}

use Step::{Js, Profiles};

const FIXTURE_FULL: &str = include_str!("../../docs/manual/fixtures/connections.debug.json");
const FIXTURE_SIMPLE: &str = include_str!("../../docs/manual/fixtures/connections.simple.json");

/// The shots, in order. Each starts from where the previous one left the windows, so the
/// main and settings windows come first (opening a connection hides main), and the query
/// shots build on each other (tabs opened by one are still open in the next).
const SHOTS: &[Shot] = &[
    Shot {
        file: "screen-layout.png",
        steps: &[
            Profiles(FIXTURE_SIMPLE),
            Js(
                WIN_MAIN,
                r#"// What this machine's Docker shows is not the manual's: an available Docker
                // with the fixture's containers.
                __shot.mock({
                  docker_available: true,
                  docker_list_containers: [
                    { id: "c1", name: "EC デモ", image: "mysql:8.0", project: null, engine: "mysql", port: 3306, networks: [], host_port: 3306, label_user: "demo", label_password: null, label_name: "EC デモ", label_ssl_mode: null },
                    { id: "c2", name: "school / mysql", image: "mysql:8.4", project: "school", engine: "mysql", port: 3306, networks: [], host_port: 13306, label_user: null, label_password: null, label_name: null, label_ssl_mode: null },
                    { id: "c3", name: "analytics / postgres", image: "postgres:17", project: "analytics", engine: "postgresql", port: 5432, networks: [], host_port: 5432, label_user: null, label_password: null, label_name: null, label_ssl_mode: null },
                    { id: "c4", name: "cache", image: "redis:7", project: null, engine: "redis", port: 6379, networks: [], host_port: 6379, label_user: null, label_password: null, label_name: null, label_ssl_mode: null },
                  ],
                  sync_folder_candidates: [
                    { service: "onedrive", folder: "C:\\Users\\you\\OneDrive", file: "C:\\Users\\you\\OneDrive\\muSQL\\profiles-sync.json", exists: false },
                    { service: "dropbox", folder: "C:\\Users\\you\\Dropbox", file: "C:\\Users\\you\\Dropbox\\muSQL\\profiles-sync.json", exists: false },
                  ],
                });
                await checkDockerAvailable();
                await refreshProfiles();
                await __shot.waitUntil(() => profileData.items.length === 3, "the simple fixture");"#,
            ),
        ],
        capture: WIN_MAIN,
    },
    Shot {
        file: "profile-list-colored.png",
        steps: &[
            Profiles(FIXTURE_FULL),
            Js(
                WIN_MAIN,
                r#"await refreshProfiles();
                await __shot.waitUntil(() => profileData.items.length === 6, "the full fixture");"#,
            ),
        ],
        capture: WIN_MAIN,
    },
    Shot {
        file: "docker-list.png",
        steps: &[Js(
            WIN_MAIN,
            r#"showDockerModal();
            await __shot.waitFor("[id=docker-modal] .db-list-item");"#,
        )],
        capture: WIN_MAIN,
    },
    Shot {
        file: "sync-settings.png",
        steps: &[Js(
            WIN_MAIN,
            r#"document.getElementById("docker-modal-close").click();
            openSyncModal();
            await __shot.waitFor(".sync-folder-item");"#,
        )],
        capture: WIN_MAIN,
    },
    Shot {
        file: "profile-edit.png",
        steps: &[
            Js(
                WIN_MAIN,
                r#"document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
                openSettings("prof-ec-prod");"#,
            ),
            Js(
                WIN_SETTINGS,
                r#"// Whether 1Password's row shows depends on this machine having `op`. The page
                // asked when it loaded, and that answer may still arrive: whatever it says,
                // the row is shown as without `op`.
                const showOpRows = updateOpRowVisibility;
                updateOpRowVisibility = () => { opAvailable = false; showOpRows(); };
                updateOpRowVisibility();
                await __shot.waitUntil(
                  () => document.getElementById("mysql-host").value === "db01.ec-prod.example.internal",
                  "the profile in the form");"#,
            ),
        ],
        capture: WIN_SETTINGS,
    },
    Shot {
        file: "ssh-settings.png",
        steps: &[Js(
            WIN_SETTINGS,
            r#"document.getElementById("ssh-section").scrollIntoView({ block: "start" });"#,
        )],
        capture: WIN_SETTINGS,
    },
    Shot {
        file: "query-sql.png",
        steps: &[
            // Before the connection opens, so nothing the page asks while opening it gets
            // this machine's answer.
            Js(
                WIN_QUERY,
                r#"__shot.mock({ op_available: false, has_ai_api_key: true });"#,
            ),
            Js(WIN_MAIN, r#"openQuery("prof-local-docker");"#),
            Js(
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
    Shot {
        file: "query-layout.png",
        steps: &[Js(
            WIN_QUERY,
            r#"for (const table of ["customers", "orders", "order_items", "customers"]) {
              openTableTab(table, "data");
              await __shot.waitFor(`.tab-pane.active table tbody tr`);
            }"#,
        )],
        capture: WIN_QUERY,
    },
    Shot {
        file: "overview.png",
        steps: &[],
        capture: WIN_QUERY,
    },
    Shot {
        file: "export-options.png",
        steps: &[Js(
            WIN_QUERY,
            r#"doExportAll("customers", ",", "csv");
            await __shot.waitFor(".modal-overlay select");"#,
        )],
        capture: WIN_QUERY,
    },
    Shot {
        file: "query-data.png",
        steps: &[Js(
            WIN_QUERY,
            r#"__shot.button(".modal-overlay", t("cancel")).click();
            openTableTab("order_items", "data");
            await __shot.waitFor(".tab-pane.active .paging-size-btn");
            __shot.button(".tab-pane.active .paging-sizes", "50").click();
            // The footer is drawn again once the 50 rows are in; until then the old one
            // (100 rows a page) is still there.
            await __shot.waitUntil(() => document.querySelector(".tab-pane.active .paging-size-btn.active")?.textContent === "50", "50 rows a page");
            __shot.button(".tab-pane.active .paging-nav", t("next")).click();
            await __shot.waitUntil(() =>
              document.querySelector(".tab-pane.active .paging-info")?.textContent.startsWith("2 /")
              && document.querySelector(".tab-pane.active table tbody td")?.textContent === "51",
              "the rows of page 2");"#,
        )],
        capture: WIN_QUERY,
    },
    Shot {
        file: "quickopen.png",
        steps: &[Js(
            WIN_QUERY,
            r#"// The SQL tab the connection opened, whatever its number.
            let sqlTab = null;
            tabManager.tabs.forEach((tab) => { if (!sqlTab && tab.type === "sql") sqlTab = tab.id; });
            tabManager.activate(sqlTab);
            openQuickOpen();
            const input = await __shot.waitFor(".quickopen-input");
            input.value = "cu";
            input.dispatchEvent(new Event("input"));
            await __shot.waitFor(".quickopen-item");"#,
        )],
        capture: WIN_QUERY,
    },
    Shot {
        file: "ai-settings.png",
        steps: &[Js(
            WIN_QUERY,
            r#"document.querySelector(".quickopen-input").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
            await __shot.waitUntil(() => !document.querySelector(".quickopen-input"), "QuickOpen to close");
            localStorage.setItem("musql:ai:provider", "claude");
            localStorage.setItem("musql:ai:model", "claude-sonnet-5");
            await showAiModal();
            await __shot.waitFor("[id=ai-modal] select");"#,
        )],
        capture: WIN_QUERY,
    },
    Shot {
        file: "ai-chat.png",
        steps: &[Js(
            WIN_QUERY,
            r#"hideAiModal();
            // A conversation already had, so no AI is called.
            localStorage.setItem(aiChatKey(currentProfileId, currentDb), JSON.stringify([
              { role: "user", content: "東京都の顧客を取得するSQL", ts: 1 },
              { role: "assistant", content: "SELECT * FROM customers WHERE prefecture = '東京都'", ts: 2 },
            ]));
            __shot.button(".tab-pane.active .actions", t("ai_assist")).click();
            await __shot.waitFor("[id=ai-assist-messages] .ai-msg.assistant");"#,
        )],
        capture: WIN_QUERY,
    },
];

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
            .any(|step| matches!(step, Js(_, script) if script.contains(DEMO_PROFILE)))
        {
            return Err(format!(
                "no shot opens {DEMO_PROFILE}, whose password is set"
            ));
        }
        std::fs::create_dir_all(out)
            .map_err(|e| format!("cannot create {}: {e}", out.display()))?;
        crate::set_password(DEMO_PROFILE, DEMO_PASSWORD)?;
        // A sync path left by an earlier run (saved from the sync settings) would merge
        // another file into the fixture profiles.
        if let Ok(file) = crate::sync_path_file(self.app) {
            let _ = std::fs::remove_file(file);
        }
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
            for step in shot.steps {
                match step {
                    Js(label, script) => self.step(label, script, false, STEP_TIMEOUT),
                    Profiles(fixture) => crate::profiles_path(self.app).and_then(|path| {
                        path.parent()
                            .map_or(Ok(()), std::fs::create_dir_all)
                            .and_then(|()| std::fs::write(&path, fixture))
                            .map_err(|e| format!("cannot write {}: {e}", path.display()))
                    }),
                }
                .map_err(|e| format!("{}: {e}", shot.file))?;
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
