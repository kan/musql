const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;

const treeEl = document.getElementById("profile-tree");
const profileNewBtn = document.getElementById("profile-new");
const groupNewBtn = document.getElementById("group-new");
const contextMenuEl = document.getElementById("context-menu");
const filterInput = document.getElementById("filter-input");
const tagFilterBarEl = document.getElementById("tag-filter-bar");
const menuBtn = document.getElementById("menu-btn");
const dockerBtn = document.getElementById("docker-btn");
const dockerModal = document.getElementById("docker-modal");
const dockerList = document.getElementById("docker-list");
const dockerModalStatus = document.getElementById("docker-modal-status");
const dockerModalClose = document.getElementById("docker-modal-close");
const dockerCredModal = document.getElementById("docker-cred-modal");
const dockerCredUser = document.getElementById("docker-cred-user");
const dockerCredPassword = document.getElementById("docker-cred-password");
const dockerCredSsl = document.getElementById("docker-cred-ssl");
const dockerCredOk = document.getElementById("docker-cred-ok");
const dockerCredCancel = document.getElementById("docker-cred-cancel");

// Apply icons to header buttons
function applyAppLabels() {
  menuBtn.innerHTML = icon('menu');
  dockerBtn.innerHTML = icon('container');
  dockerBtn.title = t('docker_btn_title');
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
const collapsedGroups = JSON.parse(localStorage.getItem("musql:collapsed") || "{}");
let dragState = null; // { type: "item"|"group", id }
let activeFilterTag = null;

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

// ── Helpers ──

// Custom confirm dialog (replaces window.confirm, which shows the origin URL in
// its title). Enter = OK, Escape / overlay click = Cancel. Resolves to a boolean.
function confirmDialog(message, opts) {
  opts = opts || {};
  return new Promise((resolve) => {
    const overlay = document.createElement("div");
    overlay.className = "modal-overlay";
    const box = document.createElement("div");
    box.className = "modal-box";
    box.style.maxWidth = "400px";

    const msg = document.createElement("p");
    msg.textContent = message;
    msg.style.whiteSpace = "pre-wrap"; // some messages contain \n
    msg.style.margin = "4px 0 0";
    box.appendChild(msg);

    const actions = document.createElement("div");
    actions.className = "actions actions-right";
    actions.style.marginTop = "16px";
    const cancelBtn = document.createElement("button");
    cancelBtn.className = "ghost";
    cancelBtn.textContent = opts.cancelLabel || t('cancel');
    const okBtn = document.createElement("button");
    okBtn.className = opts.danger ? "danger" : "success";
    okBtn.textContent = opts.okLabel || t('ok');
    actions.appendChild(cancelBtn);
    actions.appendChild(okBtn);
    box.appendChild(actions);

    overlay.appendChild(box);
    document.body.appendChild(overlay);

    let done = false;
    function finish(result) {
      if (done) return;
      done = true;
      overlay.remove();
      document.removeEventListener("keydown", onKey);
      resolve(result);
    }
    // Enter activates the focused button natively (OK is focused by default; Tab to
    // Cancel then Enter cancels) — matching window.confirm. Only Escape is handled here.
    function onKey(e) {
      if (e.key === "Escape") finish(false);
    }
    document.addEventListener("keydown", onKey);
    overlay.addEventListener("click", (e) => { if (e.target === overlay) finish(false); });
    cancelBtn.addEventListener("click", () => finish(false));
    okBtn.addEventListener("click", () => finish(true));
    okBtn.focus();
  });
}

function getItemHint(item) {
  const ssh = item.request.ssh;
  const mysqlHost = item.request.mysql.host || t('host_not_set');
  if (!ssh || !ssh.enabled) return mysqlHost;
  const sshLabel = ssh.config_host || ssh.host;
  if (!sshLabel) return mysqlHost;
  const isLocal = /^(localhost|127\.0\.0\.1|::1)$/.test(mysqlHost);
  return isLocal ? "SSH: " + sshLabel : sshLabel + " \u2192 " + mysqlHost;
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
  const nameRow = document.createElement("div");
  nameRow.className = "name-row";
  const nameEl = document.createElement("div");
  nameEl.className = "name";
  nameEl.textContent = item.name;
  nameRow.appendChild(nameEl);

  // Tag badges (inline with name)
  if (item.tags && item.tags.length > 0) {
    const tagsEl = document.createElement("div");
    tagsEl.className = "tree-item-tags";
    item.tags.forEach((tag) => {
      const badge = document.createElement("span");
      badge.className = "tree-item-tag";
      badge.textContent = tag;
      tagsEl.appendChild(badge);
    });
    nameRow.appendChild(tagsEl);
  }

  const hintEl = document.createElement("div");
  hintEl.className = "hint";
  hintEl.textContent = getItemHint(item);
  meta.appendChild(nameRow);
  meta.appendChild(hintEl);

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

  // Show empty state when no connections at all
  if (groups.length === 0 && items.length === 0) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    const emptyIcon = document.createElement("div");
    emptyIcon.className = "empty-state-icon";
    emptyIcon.innerHTML = icon('database', 40);
    empty.appendChild(emptyIcon);
    const msg = document.createElement("div");
    msg.className = "empty-state-msg";
    msg.textContent = t('empty_state_msg');
    empty.appendChild(msg);
    const btn = document.createElement("button");
    btn.innerHTML = icon('plus') + ' ' + t('empty_state_btn');
    btn.addEventListener("click", () => openSettings(""));
    empty.appendChild(btn);
    treeEl.appendChild(empty);
    return;
  }

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
  if (!(await confirmDialog(t('confirm_delete_profile'), { danger: true, okLabel: t('delete') }))) return;
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
  if (!(await confirmDialog(t('confirm_delete_group'), { danger: true, okLabel: t('delete') }))) return;
  try {
    profileData = await safeInvoke("delete_group", { id });
    renderTree();
  } catch (error) {
    alert(String(error));
  }
}

async function refreshProfiles() {
  // sync_import merges an external sync file (if configured) then returns the list;
  // with no path / unreadable file it just returns the local profiles (#47).
  profileData = await safeInvoke("sync_import");
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
  const includePasswords = await confirmDialog(t('confirm_include_passwords'));
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
      const overwrite = await confirmDialog(msg);
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

// ── Connection profile sync (external JSON, path-based) (#47) ──
let syncModalOpen = false;
async function openSyncModal() {
  if (syncModalOpen) return; // single instance
  syncModalOpen = true;
  let current = "";
  try { current = await safeInvoke("get_sync_path"); } catch (_) { /* ignore */ }

  const overlay = document.createElement("div");
  overlay.className = "modal-overlay";
  const box = document.createElement("div");
  box.className = "modal-box";
  box.style.maxWidth = "540px";

  const h2 = document.createElement("h2");
  h2.textContent = t('sync_settings');
  const headingRow = document.createElement("div");
  headingRow.className = "modal-heading-row";
  headingRow.appendChild(h2);
  headingRow.appendChild(window.createHelpButton("sync.md"));
  box.appendChild(headingRow);

  const desc = document.createElement("p");
  desc.className = "modal-status";
  desc.style.whiteSpace = "pre-wrap";
  desc.textContent = t('sync_desc');
  box.appendChild(desc);

  const label = document.createElement("label");
  const span = document.createElement("span");
  span.textContent = t('sync_path_label');
  label.appendChild(span);
  const row = document.createElement("div");
  row.style.display = "flex";
  row.style.gap = "6px";
  const input = document.createElement("input");
  input.type = "text";
  input.value = current || "";
  input.placeholder = "C:\\Users\\...\\Dropbox\\musql-profiles-sync.json";
  input.style.flex = "1";
  const browseBtn = document.createElement("button");
  browseBtn.className = "ghost";
  browseBtn.textContent = t('sync_browse');
  row.appendChild(input);
  row.appendChild(browseBtn);
  label.appendChild(row);
  box.appendChild(label);

  const status = document.createElement("p");
  status.className = "modal-status";
  box.appendChild(status);

  const actions = document.createElement("div");
  actions.className = "actions actions-right";
  actions.style.marginTop = "16px";
  const importBtn = document.createElement("button");
  importBtn.className = "ghost";
  importBtn.textContent = t('sync_import_now');
  const exportBtn = document.createElement("button");
  exportBtn.className = "ghost";
  exportBtn.textContent = t('sync_export_now');
  const cancelBtn = document.createElement("button");
  cancelBtn.className = "ghost";
  cancelBtn.textContent = t('cancel');
  const saveBtn = document.createElement("button");
  saveBtn.className = "success";
  saveBtn.textContent = t('save');
  actions.appendChild(importBtn);
  actions.appendChild(exportBtn);
  actions.appendChild(cancelBtn);
  actions.appendChild(saveBtn);
  box.appendChild(actions);

  overlay.appendChild(box);
  document.body.appendChild(overlay);

  let busy = false;
  function close() {
    overlay.remove();
    document.removeEventListener("keydown", onKey);
    syncModalOpen = false;
  }
  function onKey(e) { if (e.key === "Escape" && !busy) close(); }
  document.addEventListener("keydown", onKey);
  overlay.addEventListener("click", (e) => { if (e.target === overlay && !busy) close(); });
  cancelBtn.addEventListener("click", () => { if (!busy) close(); });

  async function persistPath() {
    await safeInvoke("set_sync_path", { path: input.value.trim() });
  }
  function setBusy(b) {
    busy = b;
    [importBtn, exportBtn, saveBtn, browseBtn, input].forEach((el) => { el.disabled = b; });
  }
  function showStatus(msg, ok) {
    status.style.color = ok ? "var(--success)" : "var(--danger)";
    status.textContent = msg;
  }

  browseBtn.addEventListener("click", async () => {
    try {
      const picked = await safeInvoke("pick_sync_path");
      if (picked) input.value = picked;
    } catch (error) { showStatus(String(error), false); }
  });

  saveBtn.addEventListener("click", async () => {
    if (busy) return;
    setBusy(true);
    try {
      await persistPath();
      // Join the sync: pull the file (merge in) then push the merged result, so a fresh
      // path gets created/populated and local-only profiles propagate out.
      profileData = await safeInvoke("sync_import");
      await safeInvoke("sync_export");
      renderTree();
      close();
    } catch (error) { showStatus(String(error), false); setBusy(false); }
  });

  importBtn.addEventListener("click", async () => {
    if (busy) return;
    setBusy(true);
    try {
      await persistPath();
      if (!input.value.trim()) { showStatus(t('sync_disabled'), false); setBusy(false); return; }
      profileData = await safeInvoke("sync_import");
      renderTree();
      showStatus(t('sync_imported'), true);
    } catch (error) { showStatus(String(error), false); }
    setBusy(false);
  });

  exportBtn.addEventListener("click", async () => {
    if (busy) return;
    setBusy(true);
    try {
      await persistPath();
      const ok = await safeInvoke("sync_export");
      showStatus(ok ? t('sync_exported') : t('sync_disabled'), ok);
    } catch (error) { showStatus(String(error), false); }
    setBusy(false);
  });

  input.focus();
}

// ── Docker ──

let dockerCredResolve = null;

async function checkDockerAvailable() {
  try {
    const available = await safeInvoke("docker_available");
    console.log("[Docker] docker_available =", available);
    dockerBtn.style.display = available ? "" : "none";
    if (available) await migrateDockerCreds();
  } catch (e) {
    console.warn("[Docker] docker_available error:", e);
    dockerBtn.style.display = "none";
  }
}

async function showDockerModal() {
  dockerModal.classList.remove("hidden");
  dockerList.innerHTML = "";
  dockerModalStatus.textContent = t('loading');
  document.getElementById("docker-modal-heading").innerHTML = icon('container', 20) + ' ' + t('docker_modal_heading');

  try {
    const containers = await safeInvoke("docker_list_containers");
    if (containers.length === 0) {
      dockerModalStatus.textContent = t('docker_no_containers');
      return;
    }
    dockerModalStatus.textContent = "";

    containers.forEach(function(c) {
      var el = document.createElement("div");
      el.className = "db-list-item";
      var iconSpan = document.createElement("span");
      iconSpan.innerHTML = icon('container', 14);
      el.appendChild(iconSpan);
      var meta = document.createElement("div");
      meta.className = "docker-item-meta";
      var nameDiv = document.createElement("div");
      nameDiv.textContent = c.name;
      meta.appendChild(nameDiv);
      var subDiv = document.createElement("div");
      subDiv.className = "docker-item-sub";
      subDiv.textContent = c.image;
      meta.appendChild(subDiv);
      el.appendChild(meta);
      el.addEventListener("click", function() { connectToDockerContainer(c); });
      dockerList.appendChild(el);
    });
  } catch (error) {
    dockerModalStatus.textContent = String(error);
  }
}

// Docker credential persistence: password in OS keyring, user/ssl_mode in localStorage
var DOCKER_CRED_KEY = "musql:docker-creds";
var DOCKER_LAST_KEY = "musql:docker-last-cred";
var DOCKER_LAST_ID = "__last__";

function loadDockerCreds() {
  try { return JSON.parse(localStorage.getItem(DOCKER_CRED_KEY) || "{}"); } catch (_) { return {}; }
}
// Migrate legacy plaintext passwords from localStorage to keyring (one-time)
async function migrateDockerCreds() {
  var all = loadDockerCreds();
  var migrated = false;
  for (var id in all) {
    if (!Object.hasOwn(all, id)) continue;
    if (all[id] && all[id].password) {
      try {
        await safeInvoke("save_docker_password", { containerId: id, password: all[id].password });
        delete all[id].password;
        migrated = true;
      } catch (_) { /* keep password in localStorage if keyring save fails */ }
    }
  }
  if (migrated) {
    localStorage.setItem(DOCKER_CRED_KEY, JSON.stringify(all));
  }
  // Also migrate last-used cred
  try {
    var last = JSON.parse(localStorage.getItem(DOCKER_LAST_KEY));
    if (last && last.password) {
      await safeInvoke("save_docker_password", { containerId: DOCKER_LAST_ID, password: last.password });
      delete last.password;
      localStorage.setItem(DOCKER_LAST_KEY, JSON.stringify(last));
    }
  } catch (_) { /* ignore */ }
}
async function saveDockerCred(containerId, cred) {
  // Save password to OS keyring (never localStorage)
  var pw = cred.password || "";
  await Promise.all([
    safeInvoke("save_docker_password", { containerId: containerId, password: pw }),
    safeInvoke("save_docker_password", { containerId: DOCKER_LAST_ID, password: pw })
  ]);
  // Save non-secret fields to localStorage
  var meta = { user: cred.user, ssl_mode: cred.ssl_mode };
  var all = loadDockerCreds();
  all[containerId] = meta;
  localStorage.setItem(DOCKER_CRED_KEY, JSON.stringify(all));
  localStorage.setItem(DOCKER_LAST_KEY, JSON.stringify(meta));
}
async function getDockerCred(containerId) {
  var all = loadDockerCreds();
  var meta = all[containerId] || null;
  var keyId = meta ? containerId : DOCKER_LAST_ID;
  if (!meta) {
    try { meta = JSON.parse(localStorage.getItem(DOCKER_LAST_KEY)); } catch (_) { /* ignore */ }
  }
  if (!meta) return null;
  var pw = await safeInvoke("get_docker_password", { containerId: keyId });
  return { user: meta.user, password: pw || "", ssl_mode: meta.ssl_mode };
}

async function showDockerCredPrompt(container) {
  // Restore saved values: per-container > last-used > label > defaults
  var saved = await getDockerCred(container.id);
  dockerCredUser.value = container.label_user || (saved && saved.user) || "root";
  dockerCredPassword.value = container.label_password || (saved && saved.password) || "";
  dockerCredSsl.value = (saved && saved.ssl_mode) || "DISABLED";
  dockerCredModal.classList.remove("hidden");
  return new Promise(function(resolve) {
    dockerCredResolve = resolve;
  });
}

dockerCredOk.addEventListener("click", function() {
  if (dockerCredResolve) {
    dockerCredResolve({ user: dockerCredUser.value, password: dockerCredPassword.value, ssl_mode: dockerCredSsl.value });
    dockerCredResolve = null;
  }
  dockerCredModal.classList.add("hidden");
});

dockerCredCancel.addEventListener("click", function() {
  if (dockerCredResolve) {
    dockerCredResolve(null);
    dockerCredResolve = null;
  }
  dockerCredModal.classList.add("hidden");
});

async function connectToDockerContainer(container) {
  var user, password, sslMode;

  if (container.label_user && container.label_password) {
    user = container.label_user;
    password = container.label_password;
    // Use saved ssl_mode if available, otherwise default
    var saved = await getDockerCred(container.id);
    sslMode = (saved && saved.ssl_mode) || "DISABLED";
  } else {
    var creds = await showDockerCredPrompt(container);
    if (!creds) return;
    user = creds.user;
    password = creds.password;
    sslMode = creds.ssl_mode;
  }

  // Persist credentials for this container
  await saveDockerCred(container.id, { user: user, password: password, ssl_mode: sslMode });

  var host, port;
  var tunnelContainerId = null;

  if (container.host_port) {
    host = "127.0.0.1";
    port = container.host_port;
  } else {
    dockerModalStatus.textContent = t('docker_creating_tunnel');
    try {
      var tunnel = await safeInvoke("docker_create_tunnel", {
        containerId: container.id,
        port: container.port
      });
      host = tunnel.local_host;
      port = tunnel.local_port;
      tunnelContainerId = tunnel.container_id;
    } catch (error) {
      dockerModalStatus.textContent = String(error);
      return;
    }
  }

  dockerModal.classList.add("hidden");

  try {
    await safeInvoke("open_docker_query_window", {
      info: {
        host: host,
        port: port,
        user: user,
        password: password,
        name: container.name,
        ssl_mode: sslMode,
        tunnel_container_id: tunnelContainerId
      }
    });
  } catch (error) {
    alert(String(error));
  }
}

dockerBtn.addEventListener("click", showDockerModal);
dockerModalClose.addEventListener("click", function() {
  dockerModal.classList.add("hidden");
});

// Check Docker availability on startup
checkDockerAvailable();

// ── Event handlers ──

profileNewBtn.addEventListener("click", () => openSettings(""));
groupNewBtn.addEventListener("click", () => createGroup());
filterInput.addEventListener("input", () => renderTree());

// Show app version
(async function() {
  try {
    var ver = await window.__TAURI__.app.getVersion();
    document.getElementById("app-version").textContent = "v" + ver;
  } catch {}
})();

refreshProfiles().catch((error) => alert(String(error)));
window.addEventListener("focus", () => { refreshProfiles().catch(() => {}); });

// Listen for profile changes from settings window
// Note: unlisten is not needed here — windows use show/hide (never destroyed),
// so JS context persists for the app lifetime and listeners are registered exactly once.
const eventApi = window.__TAURI__ && window.__TAURI__.event ? window.__TAURI__.event : null;
if (eventApi && eventApi.listen) {
  eventApi.listen("profiles:changed", () => { refreshProfiles().catch(() => {}); });

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
      case "sync-settings": openSyncModal(); break;
      case "manual": window.openManual(); break;
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
