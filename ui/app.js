const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;

const treeEl = document.getElementById("profile-tree");
const profileNewBtn = document.getElementById("profile-new");
const groupNewBtn = document.getElementById("group-new");
const importBtn = document.getElementById("import-btn");
const exportBtn = document.getElementById("export-btn");
const contextMenuEl = document.getElementById("context-menu");
const filterInput = document.getElementById("filter-input");
const tagFilterBarEl = document.getElementById("tag-filter-bar");
const menuBtn = document.getElementById("menu-btn");

// Apply icons to header buttons
function applyAppLabels() {
  menuBtn.innerHTML = icon('menu');
  importBtn.innerHTML = icon('download');
  importBtn.title = t('import_profiles_title');
  exportBtn.innerHTML = icon('upload');
  exportBtn.title = t('export_profiles_title');
  groupNewBtn.innerHTML = icon('folder-plus');
  groupNewBtn.title = t('new_group');
  profileNewBtn.innerHTML = icon('plus');
  profileNewBtn.title = t('new_profile');
}
applyAppLabels();

menuBtn.addEventListener("click", () => safeInvoke("show_popup_menu", { lang: getLang(), theme: getTheme() }));

// Search icon inside filter input
(function() {
  var wrapper = document.getElementById("filter-wrapper");
  var span = document.createElement("span");
  span.className = "filter-icon";
  span.innerHTML = icon('search', 15);
  wrapper.insertBefore(span, wrapper.firstChild);
})();

let profileData = { groups: [], items: [] };
let collapsedGroups = JSON.parse(localStorage.getItem("musql:collapsed") || "{}");
let dragState = null; // { type: "item"|"group", id }
let activeFilterTag = null;

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

// ── Helpers ──

function getItemHint(item) {
  const ssh = item.request.ssh;
  const mysqlHost = item.request.mysql.host || t('host_not_set');
  return ssh && ssh.enabled && ssh.host
    ? ssh.host + " \u2192 " + mysqlHost
    : mysqlHost;
}

function saveCollapsed() {
  localStorage.setItem("musql:collapsed", JSON.stringify(collapsedGroups));
}

// ── Rendering ──

function buildItemNode(item) {
  const el = document.createElement("div");
  el.className = "tree-node tree-item";
  el.dataset.type = "item";
  el.dataset.id = item.id;
  el.draggable = true;

  // Color bar
  if (item.color) {
    const bar = document.createElement("div");
    bar.className = "tree-item-color-bar";
    bar.style.backgroundColor = item.color;
    el.appendChild(bar);
  }

  // Connection icon
  const iconEl = document.createElement("span");
  iconEl.className = "tree-item-icon";
  iconEl.innerHTML = icon('database', 16);
  el.appendChild(iconEl);

  const meta = document.createElement("div");
  meta.className = "meta";
  const nameEl = document.createElement("div");
  nameEl.className = "name";
  nameEl.textContent = item.name;
  const hintEl = document.createElement("div");
  hintEl.className = "hint";
  hintEl.textContent = getItemHint(item);
  meta.appendChild(nameEl);
  meta.appendChild(hintEl);

  // Tag badges
  if (item.tags && item.tags.length > 0) {
    const tagsEl = document.createElement("div");
    tagsEl.className = "tree-item-tags";
    item.tags.forEach((tag) => {
      const badge = document.createElement("span");
      badge.className = "tree-item-tag";
      badge.textContent = tag;
      tagsEl.appendChild(badge);
    });
    meta.appendChild(tagsEl);
  }

  el.appendChild(meta);

  // Double-click → open query
  el.addEventListener("dblclick", (e) => {
    e.preventDefault();
    openQuery(item.id);
  });

  // Right-click → context menu
  el.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    e.stopPropagation();
    showContextMenu(e, [
      { label: t('ctx_open'), icon: "external-link", action: () => openQuery(item.id) },
      { label: t('ctx_settings'), icon: "settings", action: () => openSettings(item.id) },
      { separator: true },
      { label: t('ctx_duplicate'), icon: "copy", action: () => duplicateProfile(item.id) },
      { label: t('ctx_delete'), icon: "trash-2", danger: true, action: () => deleteProfile(item.id) },
    ]);
  });

  // Drag
  el.addEventListener("dragstart", (e) => {
    e.stopPropagation();
    dragState = { type: "item", id: item.id };
    el.classList.add("dragging");
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", item.id);
  });
  el.addEventListener("dragend", () => {
    dragState = null;
    el.classList.remove("dragging");
    clearDropIndicators();
  });

  return el;
}

function buildGroupNode(group, children, forceExpand) {
  const collapsed = !forceExpand && collapsedGroups[group.id];
  const wrapper = document.createElement("div");
  wrapper.className = "tree-node tree-group" + (collapsed ? " collapsed" : "");
  wrapper.dataset.type = "group";
  wrapper.dataset.id = group.id;
  wrapper.draggable = true;

  const header = document.createElement("div");
  header.className = "tree-group-header";
  header.dataset.type = "group-header";
  header.dataset.id = group.id;

  const toggle = document.createElement("span");
  toggle.className = "toggle";
  toggle.textContent = collapsed ? "\u25B6" : "\u25BC";
  header.appendChild(toggle);

  const folderIcon = document.createElement("span");
  folderIcon.className = "tree-group-icon";
  folderIcon.innerHTML = icon(collapsed ? 'folder' : 'folder-open', 16);
  header.appendChild(folderIcon);

  const nameSpan = document.createElement("span");
  nameSpan.textContent = group.name;
  header.appendChild(nameSpan);

  const countSpan = document.createElement("span");
  countSpan.className = "group-count";
  countSpan.textContent = "(" + children.length + ")";
  header.appendChild(countSpan);

  // Click toggle
  header.addEventListener("click", () => {
    collapsedGroups[group.id] = !collapsedGroups[group.id];
    saveCollapsed();
    renderTree();
  });

  // Right-click → group context menu
  header.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    e.stopPropagation();
    showContextMenu(e, [
      { label: t('ctx_add_setting'), icon: "plus", action: () => openSettings("", group.id) },
      { separator: true },
      { label: t('ctx_rename'), icon: "pencil", action: () => renameGroup(group.id, group.name) },
      { label: t('ctx_delete'), icon: "trash-2", danger: true, action: () => deleteGroup(group.id) },
    ]);
  });

  wrapper.appendChild(header);

  const childrenEl = document.createElement("div");
  childrenEl.className = "tree-group-children";
  children.sort((a, b) => a.order - b.order);
  children.forEach((item) => childrenEl.appendChild(buildItemNode(item)));
  wrapper.appendChild(childrenEl);

  // Group drag
  wrapper.addEventListener("dragstart", (e) => {
    if (dragState) return; // item drag already captured
    dragState = { type: "group", id: group.id };
    wrapper.classList.add("dragging");
    e.dataTransfer.effectAllowed = "move";
    e.dataTransfer.setData("text/plain", group.id);
  });
  wrapper.addEventListener("dragend", () => {
    dragState = null;
    wrapper.classList.remove("dragging");
    clearDropIndicators();
  });

  return wrapper;
}

function itemMatchesFilter(item, query) {
  // Tag filter chip
  if (activeFilterTag) {
    if (!item.tags || !item.tags.includes(activeFilterTag)) return false;
  }
  if (!query) return true;
  const q = query.toLowerCase();
  if (item.name.toLowerCase().includes(q)) return true;
  if (getItemHint(item).toLowerCase().includes(q)) return true;
  if (item.tags && item.tags.some((t) => t.toLowerCase().includes(q))) return true;
  return false;
}

function renderTagFilterBar() {
  tagFilterBarEl.innerHTML = "";
  const tags = new Set();
  (profileData.items || []).forEach((item) => {
    if (item.tags) item.tags.forEach((t) => tags.add(t));
  });
  if (tags.size === 0) {
    tagFilterBarEl.classList.add("hidden");
    return;
  }
  tagFilterBarEl.classList.remove("hidden");
  [...tags].sort().forEach((tag) => {
    const chip = document.createElement("span");
    chip.className = "tag-filter-chip" + (activeFilterTag === tag ? " active" : "");
    chip.innerHTML = icon('search', 12);
    chip.appendChild(document.createTextNode(' ' + tag));
    chip.addEventListener("click", () => {
      activeFilterTag = activeFilterTag === tag ? null : tag;
      renderTagFilterBar();
      renderTree();
    });
    tagFilterBarEl.appendChild(chip);
  });
}

function renderTree() {
  treeEl.innerHTML = "";
  renderTagFilterBar();

  const groups = profileData.groups || [];
  const items = profileData.items || [];
  const query = filterInput.value.trim();
  const filtering = query.length > 0 || activeFilterTag != null;

  // Build top-level entries: root items (group_id == null) and groups, interleaved by order
  const rootItems = items.filter((it) => !it.group_id && itemMatchesFilter(it, query));
  const entries = [];
  groups.forEach((g) => {
    const children = items.filter((it) => it.group_id === g.id && itemMatchesFilter(it, query));
    // When filtering, skip groups with no matching children
    if (filtering && children.length === 0) return;
    entries.push({ kind: "group", data: g, children, order: g.order });
  });
  rootItems.forEach((it) => entries.push({ kind: "item", data: it, order: it.order }));
  entries.sort((a, b) => a.order - b.order);

  entries.forEach((entry) => {
    if (entry.kind === "group") {
      treeEl.appendChild(buildGroupNode(entry.data, entry.children, filtering));
    } else {
      treeEl.appendChild(buildItemNode(entry.data));
    }
  });

  setupDropZones();
}

// ── Actions ──

function openSettings(id, groupId) {
  safeInvoke("open_settings_window", { id: id || null, groupId: groupId || null })
    .catch((error) => alert(String(error)));
}

function openQuery(id) {
  safeInvoke("open_query_window", { id })
    .catch((error) => alert(String(error)));
}

async function duplicateProfile(id) {
  try {
    profileData = await safeInvoke("duplicate_profile", { id });
    renderTree();
  } catch (error) {
    alert(String(error));
  }
}

async function deleteProfile(id) {
  if (!confirm(t('confirm_delete_profile'))) return;
  try {
    profileData = await safeInvoke("delete_profile", { id });
    renderTree();
  } catch (error) {
    alert(String(error));
  }
}

async function createGroup() {
  const name = prompt(t('prompt_group_name'));
  if (!name || !name.trim()) return;
  try {
    profileData = await safeInvoke("save_group", { id: null, name: name.trim() });
    renderTree();
  } catch (error) {
    alert(String(error));
  }
}

async function renameGroup(id, currentName) {
  const name = prompt(t('prompt_rename_group'), currentName);
  if (!name || !name.trim()) return;
  try {
    profileData = await safeInvoke("save_group", { id, name: name.trim() });
    renderTree();
  } catch (error) {
    alert(String(error));
  }
}

async function deleteGroup(id) {
  if (!confirm(t('confirm_delete_group'))) return;
  try {
    profileData = await safeInvoke("delete_group", { id });
    renderTree();
  } catch (error) {
    alert(String(error));
  }
}

async function refreshProfiles() {
  profileData = await safeInvoke("list_profiles");
  renderTree();
}

// ── Context menu ──

function showContextMenu(e, menuItems) {
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
      var iconSpan = document.createElement('span');
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
  contextMenuEl.classList.add("hidden");
}

document.addEventListener("click", hideContextMenu);
document.addEventListener("contextmenu", (e) => {
  // Only hide if not on a tree node (tree nodes handle their own)
  if (!e.target.closest(".tree-node")) hideContextMenu();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") hideContextMenu();
});

// ── Drag & Drop ──

function clearDropIndicators() {
  treeEl.querySelectorAll(".drop-before,.drop-after,.drop-into").forEach((el) => {
    el.classList.remove("drop-before", "drop-after", "drop-into");
  });
  treeEl.classList.remove("drop-root");
}

function getDropZone(e, el) {
  const rect = el.getBoundingClientRect();
  const y = e.clientY - rect.top;
  const ratio = y / rect.height;
  const isGroupHeader = el.dataset.type === "group-header";

  if (isGroupHeader) {
    if (ratio < 0.3) return "before";
    if (ratio > 0.7) return "after";
    return "into";
  }
  return ratio < 0.5 ? "before" : "after";
}

function setupDropZones() {
  // Top-level drop targets: tree-items (root) and tree-group-headers
  const dropTargets = treeEl.querySelectorAll(".tree-item, .tree-group-header");

  dropTargets.forEach((target) => {
    target.addEventListener("dragover", (e) => {
      if (!dragState) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      clearDropIndicators();

      const zone = getDropZone(e, target);
      if (zone === "into") target.classList.add("drop-into");
      else if (zone === "before") target.classList.add("drop-before");
      else target.classList.add("drop-after");
    });

    target.addEventListener("dragleave", () => {
      target.classList.remove("drop-before", "drop-after", "drop-into");
    });

    target.addEventListener("drop", (e) => {
      e.preventDefault();
      if (!dragState) return;

      const zone = getDropZone(e, target);
      clearDropIndicators();

      handleDrop(target, zone);
    });
  });

  // Also allow dropping on group-children areas (for empty groups)
  treeEl.querySelectorAll(".tree-group-children").forEach((container) => {
    container.addEventListener("dragover", (e) => {
      if (!dragState) return;
      // Only act if the container is empty or if not over a child
      if (e.target === container) {
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        const header = container.previousElementSibling;
        if (header) {
          clearDropIndicators();
          header.classList.add("drop-into");
        }
      }
    });
    container.addEventListener("drop", (e) => {
      if (!dragState) return;
      if (e.target === container) {
        e.preventDefault();
        clearDropIndicators();
        const groupId = container.closest(".tree-group")?.dataset.id;
        if (groupId) handleDropIntoGroup(groupId);
      }
    });
  });

  // Allow dropping on the tree container itself (empty space) to move to root
  treeEl.addEventListener("dragover", (e) => {
    if (!dragState) return;
    // Only act when hovering directly on the tree background, not on a child node
    const overNode = e.target.closest(".tree-item, .tree-group-header, .tree-group-children");
    if (overNode) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    clearDropIndicators();
    treeEl.classList.add("drop-root");
  });
  treeEl.addEventListener("dragleave", (e) => {
    if (e.target === treeEl || !treeEl.contains(e.relatedTarget)) {
      treeEl.classList.remove("drop-root");
    }
  });
  treeEl.addEventListener("drop", (e) => {
    if (!dragState) return;
    const overNode = e.target.closest(".tree-item, .tree-group-header, .tree-group-children");
    if (overNode) return;
    e.preventDefault();
    treeEl.classList.remove("drop-root");
    clearDropIndicators();
    handleDropToRoot();
  });
}

function handleDrop(target, zone) {
  const targetType = target.dataset.type; // "item" or "group-header"
  const targetId = target.dataset.id;

  if (dragState.type === "group") {
    // Group can only reorder among top-level (before/after other top-level nodes)
    handleGroupReorder(targetType, targetId, zone);
  } else {
    // Item
    handleItemDrop(targetType, targetId, zone);
  }
}

function handleDropToRoot() {
  if (!dragState || dragState.type !== "item") return;
  const movedItem = profileData.items.find((it) => it.id === dragState.id);
  if (!movedItem) return;
  movedItem.group_id = null;
  // Place at the end of top-level
  const entries = [];
  profileData.groups.forEach((g) => entries.push(g.order));
  profileData.items.filter((it) => !it.group_id && it.id !== movedItem.id).forEach((it) => entries.push(it.order));
  const maxOrder = entries.length > 0 ? Math.max(...entries) : 0;
  movedItem.order = maxOrder + 1000;
  commitReorder();
}

function handleDropIntoGroup(groupId) {
  if (!dragState || dragState.type !== "item") return;
  // Move item to end of group
  const item = profileData.items.find((it) => it.id === dragState.id);
  if (!item) return;
  const groupChildren = profileData.items.filter((it) => it.group_id === groupId);
  const maxOrder = groupChildren.reduce((m, c) => Math.max(m, c.order), 0);
  item.group_id = groupId;
  item.order = maxOrder + 1000;
  commitReorder();
}

function handleItemDrop(targetType, targetId, zone) {
  const movedItem = profileData.items.find((it) => it.id === dragState.id);
  if (!movedItem) return;

  if (zone === "into" && targetType === "group-header") {
    // Drop into group
    const groupChildren = profileData.items.filter((it) => it.group_id === targetId);
    const maxOrder = groupChildren.reduce((m, c) => Math.max(m, c.order), 0);
    movedItem.group_id = targetId;
    movedItem.order = maxOrder + 1000;
    commitReorder();
    return;
  }

  if (targetType === "item") {
    const targetItem = profileData.items.find((it) => it.id === targetId);
    if (!targetItem) return;
    // Move item to same container as target
    movedItem.group_id = targetItem.group_id || null;
    // Reorder: collect siblings, insert at position
    const siblings = profileData.items
      .filter((it) => (it.group_id || null) === (movedItem.group_id || null) && it.id !== movedItem.id)
      .sort((a, b) => a.order - b.order);
    const idx = siblings.findIndex((s) => s.id === targetId);
    const insertAt = zone === "before" ? idx : idx + 1;
    siblings.splice(insertAt, 0, movedItem);
    siblings.forEach((s, i) => { s.order = (i + 1) * 1000; });
    commitReorder();
  } else if (targetType === "group-header") {
    // Before/after a group at top level
    movedItem.group_id = null;
    // Position among top-level entries
    const targetGroup = profileData.groups.find((g) => g.id === targetId);
    if (!targetGroup) return;
    const ref = targetGroup.order;
    movedItem.order = zone === "before" ? ref - 1 : ref + 1;
    renumberTopLevel();
    commitReorder();
  }
}

function handleGroupReorder(targetType, targetId, zone) {
  const movedGroup = profileData.groups.find((g) => g.id === dragState.id);
  if (!movedGroup) return;

  if (targetType === "group-header" && zone === "into") {
    // Can't put group into group, treat as "after"
    return;
  }

  // Get top-level order reference
  let refOrder;
  if (targetType === "group-header") {
    const tg = profileData.groups.find((g) => g.id === targetId);
    if (tg) refOrder = tg.order;
  } else {
    const ti = profileData.items.find((it) => it.id === targetId);
    if (ti) refOrder = ti.order;
  }
  if (refOrder == null) return;

  movedGroup.order = zone === "before" ? refOrder - 1 : refOrder + 1;
  renumberTopLevel();
  commitReorder();
}

function renumberTopLevel() {
  // Collect all top-level entries, sort, re-assign sequential orders
  const entries = [];
  profileData.groups.forEach((g) => entries.push({ kind: "group", ref: g }));
  profileData.items.filter((it) => !it.group_id).forEach((it) => entries.push({ kind: "item", ref: it }));
  entries.sort((a, b) => a.ref.order - b.ref.order);
  entries.forEach((e, i) => { e.ref.order = (i + 1) * 1000; });
}

async function commitReorder() {
  const profilePatches = profileData.items.map((it) => ({
    id: it.id,
    group_id: it.group_id || null,
    order: it.order,
  }));
  const groupPatches = profileData.groups.map((g) => ({
    id: g.id,
    order: g.order,
  }));
  try {
    profileData = await safeInvoke("reorder", {
      profilePatches,
      groupPatches,
    });
    renderTree();
  } catch (error) {
    alert(String(error));
    refreshProfiles();
  }
}

// ── Import / Export ──

async function exportProfiles() {
  const includePasswords = confirm(t('confirm_include_passwords'));
  try {
    await safeInvoke("export_profiles", { includePasswords });
  } catch (error) {
    alert(String(error));
  }
}

async function importProfiles() {
  try {
    const result = await safeInvoke("import_profiles", {
      mode: null, filePath: null
    });
    if (!result) return; // cancelled

    if (result.conflicts) {
      // Duplicates found — ask user
      const msg = t('import_conflicts', {
        groups: result.conflicts.groups.length,
        profiles: result.conflicts.profiles.length,
      });
      const overwrite = confirm(msg);
      const mode = overwrite ? "overwrite" : "add";
      const result2 = await safeInvoke("import_profiles", {
        mode, filePath: result.file_path,
      });
      if (result2) {
        profileData = result2;
        renderTree();
        alert(t('import_success', { n: result2.imported_count }));
      }
    } else {
      // No duplicates — already imported
      profileData = result;
      renderTree();
      alert(t('import_success', { n: result.imported_count }));
    }
  } catch (error) {
    alert(String(error));
  }
}

// ── Event handlers ──

exportBtn.addEventListener("click", () => exportProfiles());
importBtn.addEventListener("click", () => importProfiles());
profileNewBtn.addEventListener("click", () => openSettings(""));
groupNewBtn.addEventListener("click", () => createGroup());
filterInput.addEventListener("input", () => renderTree());

// Show app version
(async function() {
  try {
    var ver = await window.__TAURI__.app.getVersion();
    document.getElementById("app-version").textContent = "v" + ver;
  } catch(e) {}
})();

refreshProfiles().catch((error) => alert(String(error)));
window.addEventListener("focus", () => refreshProfiles());

// Listen for profile changes from settings window
const eventApi = window.__TAURI__ && window.__TAURI__.event ? window.__TAURI__.event : null;
if (eventApi && eventApi.listen) {
  eventApi.listen("profiles:changed", () => refreshProfiles());

  // Update banner
  eventApi.listen("update-available", (event) => {
    var version = event.payload && event.payload.version;
    if (!version) return;
    var existing = document.querySelector(".update-banner");
    if (existing) return;
    var banner = document.createElement("div");
    banner.className = "update-banner";
    var textEl = document.createElement("span");
    textEl.className = "update-banner-text";
    textEl.textContent = t("update_available", { version: version });
    banner.appendChild(textEl);
    var btn = document.createElement("button");
    btn.textContent = t("update_install");
    btn.addEventListener("click", function() {
      btn.disabled = true;
      btn.textContent = t("update_installing");
      safeInvoke("install_update").catch(function(e) {
        btn.disabled = false;
        btn.textContent = t("update_install");
        alert(String(e));
      });
    });
    banner.appendChild(btn);
    var main = document.querySelector("main");
    main.insertBefore(banner, main.children[1]);
  });

  eventApi.listen("menu:action", (event) => {
    switch (event.payload) {
      case "new-profile": openSettings(""); break;
      case "new-group": createGroup(); break;
      case "import": importProfiles(); break;
      case "export": exportProfiles(); break;
      case "theme-light": setTheme("light"); break;
      case "theme-dark": setTheme("dark"); break;
      case "lang-en": setLang("en"); break;
      case "lang-ja": setLang("ja"); break;
      case "check-update":
        safeInvoke("check_update").then(function(hasUpdate) {
          if (!hasUpdate) alert(t("no_update_available"));
        }).catch(function(e) { alert(String(e)); });
        break;
      case "no-update":
        alert(t("no_update_available"));
        break;
    }
  });
}

// Re-render on language change
window.addEventListener("musql:langchange", () => {
  applyAppLabels();
  applyI18n();
  renderTree();
});
