(function() {
  var STORAGE_KEY = "musql:theme";

  function getPreferred() {
    var saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "dark" || saved === "light") return saved;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  function apply(theme) {
    document.documentElement.classList.toggle("dark", theme === "dark");
  }

  function setTheme(theme) {
    if (theme !== "dark" && theme !== "light") return;
    localStorage.setItem(STORAGE_KEY, theme);
    apply(theme);
    window.dispatchEvent(new CustomEvent("musql:themechange"));
  }

  // Expose globals
  window.getTheme = getPreferred;
  window.setTheme = setTheme;

  // Apply immediately to prevent FOUC
  apply(getPreferred());

  // Sync across windows via storage event
  window.addEventListener("storage", function(e) {
    if (e.key === STORAGE_KEY) {
      apply(getPreferred());
      window.dispatchEvent(new CustomEvent("musql:themechange"));
    }
  });

  // Also listen for system theme changes (when no manual override)
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", function() {
    if (!localStorage.getItem(STORAGE_KEY)) apply(getPreferred());
  });

  // Theme toggle is now menu-only; no floating button.

  // Suppress WebView reload shortcuts (Ctrl+R / Ctrl+Shift+R / Ctrl+F5 / F5) in every
  // window. A reload serves no purpose in a packaged app and, in the query window, would
  // drop in-flight query tracking (RUNNING_QUERIES) and unsaved tab state (#42).
  window.addEventListener("keydown", function(e) {
    var isReloadKey = e.key === "F5" ||
      ((e.ctrlKey || e.metaKey) && (e.key === "r" || e.key === "R"));
    if (isReloadKey) e.preventDefault();
  }, { capture: true });
})();
