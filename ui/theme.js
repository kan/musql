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

  // Apply immediately to prevent FOUC
  apply(getPreferred());

  // Sync across windows via storage event
  window.addEventListener("storage", function(e) {
    if (e.key === STORAGE_KEY) apply(getPreferred());
  });

  // Also listen for system theme changes (when no manual override)
  window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", function() {
    if (!localStorage.getItem(STORAGE_KEY)) apply(getPreferred());
  });

  // Build toggle button after DOM ready (main window only)
  document.addEventListener("DOMContentLoaded", function() {
    if (!document.body.hasAttribute("data-theme-toggle")) return;

    var btn = document.createElement("button");
    btn.className = "theme-toggle";
    btn.title = typeof t === "function" ? t("toggle_dark_mode") : "Toggle dark mode";

    function updateIcon() {
      var isDark = document.documentElement.classList.contains("dark");
      btn.innerHTML = typeof icon === "function" ? icon(isDark ? "sun" : "moon", 18) : (isDark ? "\u2600" : "\u263D");
    }

    btn.addEventListener("click", function() {
      var current = getPreferred();
      var next = current === "dark" ? "light" : "dark";
      localStorage.setItem(STORAGE_KEY, next);
      apply(next);
      updateIcon();
    });

    updateIcon();
    document.body.appendChild(btn);

    // Update title when language changes
    window.addEventListener("musql:langchange", function() {
      btn.title = typeof t === "function" ? t("toggle_dark_mode") : "Toggle dark mode";
    });
  });
})();
