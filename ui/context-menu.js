// The context menu shared by the windows (main, query, Redis #98). The page holds the
// element (`<div id="context-menu" class="context-menu hidden">`); a click anywhere or
// Escape hides it (registered below). A button that opens the menu stops its click from
// reaching the document. Needs icons.js (`icon`).

// `menuItems`: { label, icon?, danger?, action } or { separator: true }. Opens at the
// event's position, moved back inside the window when it would overflow.
function showContextMenu(e, menuItems) {
  const contextMenuEl = document.getElementById("context-menu");
  hideContextMenu();
  contextMenuEl.innerHTML = "";
  menuItems.forEach((mi) => {
    if (mi.separator) {
      const sep = document.createElement("div");
      sep.className = "context-menu-separator";
      contextMenuEl.appendChild(sep);
      return;
    }
    const el = document.createElement("div");
    el.className = "context-menu-item";
    if (mi.icon) {
      const iconSpan = document.createElement('span');
      iconSpan.innerHTML = icon(mi.icon);
      el.appendChild(iconSpan);
      el.appendChild(document.createTextNode(' ' + mi.label));
    } else {
      el.textContent = mi.label;
    }
    if (mi.danger) el.style.color = "var(--danger)";
    el.addEventListener("click", () => {
      hideContextMenu();
      mi.action();
    });
    contextMenuEl.appendChild(el);
  });

  contextMenuEl.classList.remove("hidden");
  contextMenuEl.style.left = e.clientX + "px";
  contextMenuEl.style.top = e.clientY + "px";

  // Adjust if overflows viewport
  requestAnimationFrame(() => {
    const rect = contextMenuEl.getBoundingClientRect();
    if (rect.right > window.innerWidth) {
      contextMenuEl.style.left = (window.innerWidth - rect.width - 4) + "px";
    }
    if (rect.bottom > window.innerHeight) {
      contextMenuEl.style.top = (window.innerHeight - rect.height - 4) + "px";
    }
  });
}

function hideContextMenu() {
  document.getElementById("context-menu").classList.add("hidden");
}

document.addEventListener("click", hideContextMenu);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") hideContextMenu();
});

window.showContextMenu = showContextMenu;
window.hideContextMenu = hideContextMenu;
