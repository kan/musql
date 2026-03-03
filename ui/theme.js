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
})();
