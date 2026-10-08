// Sends uncaught frontend errors to the application log file (#110). The file is
// written by Rust (`src-tauri/src/app_log.rs`); a release build has no DevTools, so
// this is the only place a JS error can be read afterwards.
//
// Loaded first on every page so that errors in the scripts after it are caught too.
//
// **Only `Error` objects are written.** A rejected `invoke` carries the Rust command's
// `Err` string, and for a query that string quotes part of the SQL. Every such call is
// caught today, but one missing `catch` must not be enough to put SQL in the log, so a
// non-Error value is recorded by its type alone.
(function () {
  // Skip repeats of the same error inside this window, so one that fires on every
  // keystroke cannot fill the file.
  const REPEAT_WINDOW_MS = 60000;
  // How many distinct errors to remember. The oldest is forgotten first.
  const MAX_KEYS = 100;
  // Hard cap per window, whatever the keys are: an error whose message carries a
  // changing value (a row id, a coordinate) is a new key every time.
  const MAX_PER_WINDOW = 20;

  const seen = new Map();
  let windowStart = 0;
  let sentInWindow = 0;

  // Returns null when the error should be skipped, otherwise how many repeats were
  // skipped since it was last written.
  function throttle(key) {
    const now = Date.now();
    const prev = seen.get(key);
    if (prev && now - prev.at < REPEAT_WINDOW_MS) {
      prev.suppressed++;
      return null;
    }
    if (now - windowStart >= REPEAT_WINDOW_MS) {
      windowStart = now;
      sentInWindow = 0;
    }
    if (sentInWindow >= MAX_PER_WINDOW) return null;
    sentInWindow++;
    // Re-insert to move the key to the end (the first key of a Map is the oldest).
    seen.delete(key);
    seen.set(key, { at: now, suppressed: 0 });
    if (seen.size > MAX_KEYS) seen.delete(seen.keys().next().value);
    return prev ? prev.suppressed : 0;
  }

  function describe(err) {
    if (!(err instanceof Error)) return "(non-Error value of type " + typeof err + ", content not logged)";
    const head = err.name + ": " + err.message;
    if (!err.stack) return head;
    return err.stack.startsWith(head) ? err.stack : head + "\n" + err.stack;
  }

  function report(source, err) {
    // The first line (`Name: message`) is the throttle key, so the same exception thrown
    // from different call sites counts as one.
    const text = describe(err);
    const suppressed = throttle(source + ": " + text.split("\n", 1)[0]);
    if (suppressed === null) return;
    let message = source + ": " + text;
    if (suppressed > 0) message += "\n(" + suppressed + " repeats suppressed)";
    const core = window.__TAURI__ && window.__TAURI__.core;
    if (!core) return;
    // A failure to log must not raise another unhandled rejection.
    core.invoke("log_frontend", { message }).catch(() => {});
  }

  window.addEventListener("error", (e) => {
    // `error` is missing for script load failures; the location is all there is then.
    report("window.onerror", e.error || new Error("script error at " + e.filename + ":" + e.lineno + ":" + e.colno));
  });
  window.addEventListener("unhandledrejection", (e) => report("unhandledrejection", e.reason));
})();
