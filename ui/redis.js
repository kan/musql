// The Redis window (#98): the keys on the left, and on the right a CLI tab plus one tab
// per opened key. One connection at a time, held by Rust (`redis_connect`).

const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;
const currentWindow = window.__TAURI__ && window.__TAURI__.window
  ? window.__TAURI__.window.getCurrentWindow()
  : null;
const eventApi = window.__TAURI__ && window.__TAURI__.event ? window.__TAURI__.event : null;

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

// ── DOM refs ──
const statusEl = document.getElementById("redis-status");
const explorerEl = document.getElementById("explorer");
const serverEl = document.getElementById("redis-server");
const refreshBtn = document.getElementById("redis-refresh-btn");
const patternInput = document.getElementById("redis-pattern");
const keyListEl = document.getElementById("redis-key-list");
const moreBtn = document.getElementById("redis-more-btn");
const tabBarTabs = document.getElementById("tab-bar-tabs");
const tabContent = document.getElementById("tab-content");
const menuBtn = document.getElementById("menu-btn");

menuBtn.innerHTML = icon("menu");
refreshBtn.innerHTML = icon("rotate-cw");
menuBtn.addEventListener("click", () =>
  safeInvoke("show_popup_menu", { lang: getLang(), theme: getTheme() }));

// ── State ──
let profileId = null;
let profileName = "";
// The socat tunnel container of a Docker connection (`openDocker`), stopped on close.
let dockerTunnelContainerId = null;
let serverVersion = "";
let currentDb = 0;
// A string: SCAN cursors are u64, which a JS number can round (`redis_client::ScanPage`).
let scanCursor = "0";
// Bumped when the window is cleared, so a reply that arrives after the window was closed
// is dropped instead of drawn into the new state.
let generation = 0;
// Bumped when the key list starts over (a new pattern, a refresh, SELECT), so an earlier
// listing still on its way does not add its keys to the new list.
let listing = 0;

// The CLI output keeps this many lines; older ones are dropped.
const CLI_LINE_LIMIT = 1000;

// ── Status and title ──

function showStatus(text) {
  statusEl.textContent = text;
  statusEl.classList.toggle("hidden", !text);
}

function updateWindowTitle() {
  const title = profileName ? profileName + " / DB " + currentDb + " — muSQL" : "muSQL Redis";
  // Through Rust, not the JS setTitle: the dev build marker is added there.
  safeInvoke("set_window_title", { title }).catch((e) => console.warn("set_window_title failed:", e));
}

function updateServerLabel() {
  serverEl.textContent = t("redis_server_label", { version: serverVersion, db: currentDb }) + " ▾";
}

// The DB in use changed (SELECT in the CLI, or the DB switcher): follow it in the title
// and the keys. The key tabs belong to the old DB (their reload would read the new DB's
// key of the same name), so they are closed.
function applyDb(db) {
  if (db === currentDb) return;
  currentDb = db;
  for (const id of [...tabs.keys()]) if (id !== "cli") dropTab(id);
  activateTab("cli");
  updateServerLabel();
  updateWindowTitle();
  loadKeys(true);
}

// The DB switcher: the label under the window's top left opens the server's DBs.
async function openDbMenu() {
  const gen = generation;
  let dbs;
  try {
    dbs = await safeInvoke("redis_databases");
  } catch (error) {
    if (gen === generation) showStatus(String(error));
    return;
  }
  if (gen !== generation) return;
  const rect = serverEl.getBoundingClientRect();
  showContextMenu(
    { clientX: rect.left, clientY: rect.bottom },
    dbs.map((d) => ({
      label: (d.db === currentDb ? "✓ " : " ") + t("redis_db_item", { db: d.db, keys: d.keys }),
      action: () => selectDb(d.db),
    })),
  );
}

async function selectDb(db) {
  const gen = generation;
  try {
    await safeInvoke("redis_select", { db });
    if (gen !== generation) return;
    showStatus("");
    applyDb(db);
  } catch (error) {
    if (gen === generation) showStatus(String(error));
  }
}

// ── Tabs ──

const tabs = new Map(); // id → { el, pane }
let activeTabId = null;

function activateTab(id) {
  for (const [tabId, tab] of tabs) {
    tab.el.classList.toggle("active", tabId === id);
    tab.pane.classList.toggle("active", tabId === id);
  }
  activeTabId = id;
}

function addTab(id, title, iconName, closable) {
  const existing = tabs.get(id);
  if (existing) {
    activateTab(id);
    return existing;
  }
  const el = document.createElement("div");
  el.className = "tab-item";
  const tabIcon = document.createElement("span");
  tabIcon.innerHTML = icon(iconName, 14);
  tabIcon.style.display = "flex";
  el.appendChild(tabIcon);
  const label = document.createElement("span");
  label.textContent = title;
  el.appendChild(label);
  if (closable) {
    const closeBtn = document.createElement("span");
    closeBtn.className = "tab-close";
    closeBtn.innerHTML = icon("x", 14);
    closeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      closeTab(id);
    });
    el.appendChild(closeBtn);
  }
  el.addEventListener("click", () => activateTab(id));
  tabBarTabs.appendChild(el);

  const pane = document.createElement("div");
  pane.className = "tab-pane";
  tabContent.appendChild(pane);

  const tab = { el, pane };
  tabs.set(id, tab);
  activateTab(id);
  return tab;
}

function dropTab(id) {
  const tab = tabs.get(id);
  if (!tab) return;
  tab.el.remove();
  tab.pane.remove();
  tabs.delete(id);
}

function closeTab(id) {
  dropTab(id);
  if (activeTabId === id) activateTab("cli");
}

function removeAllTabs() {
  for (const id of [...tabs.keys()]) dropTab(id);
  activeTabId = null;
}

// ── Keys ──

// The pattern of the listing on screen: "Load more" goes on with it.
let scanPattern = "";

// The box's text as a SCAN pattern. Text without a wildcard (`*`, `?`, `[`) matches keys
// that contain it, so typing part of a name is enough.
function patternFor(text) {
  const s = text.trim();
  if (!s || /[*?[]/.test(s)) return s;
  return "*" + s + "*";
}

// Shows the next keys. Rust takes as many SCAN steps as it needs for about a screenful
// (`redis_client::scan`); `reset` starts over with the pattern box's value.
async function loadKeys(reset) {
  if (reset) {
    listing++;
    scanCursor = "0";
    scanPattern = patternFor(patternInput.value);
    clearKeyList();
  }
  const current = listing;
  moreBtn.classList.add("hidden");
  let failed = false;
  try {
    const page = await safeInvoke("redis_scan", { cursor: scanCursor, pattern: scanPattern });
    if (current !== listing) return;
    showStatus("");
    keyListEl.querySelector(".redis-empty")?.remove();
    scanCursor = page.cursor;
    addKeys(page.keys);
  } catch (error) {
    if (current === listing) showStatus(String(error));
    // The cursor did not move: "Load more" tries the same step again.
    failed = true;
  }
  if (current !== listing) return;
  if (!failed && keyListEl.children.length === 0) {
    const empty = document.createElement("div");
    empty.className = "redis-empty";
    empty.textContent = t("redis_no_keys");
    keyListEl.appendChild(empty);
  }
  moreBtn.classList.toggle("hidden", scanCursor === "0");
}

// The list is grouped by type, in this order (a module's type, such as ReJSON-RL, comes
// after them, by name). Each group lists its keys by name. The counts are of the keys
// loaded so far: SCAN returns the keys in no order, so a group grows with "Load more".
const KIND_ORDER = ["string", "hash", "list", "set", "zset", "stream"];
const keyGroups = new Map(); // kind → { el, count, body, entries }
// The keys listed (`keyId`): SCAN may return a key twice.
const listedKeys = new Set();

// A key's identity: its bytes when the name is binary (`raw`), else its name.
function keyId(entry) {
  return entry.raw ? "raw:" + entry.raw : "key:" + entry.key;
}
// The groups the user folded, kept for the connection (a reload keeps them folded).
const foldedKinds = new Set();

function kindRank(kind) {
  const i = KIND_ORDER.indexOf(kind);
  return i >= 0 ? i : KIND_ORDER.length;
}

// By code unit, as the names are shown (localeCompare would also fold case and accents).
function compareText(a, b) {
  return a < b ? -1 : a > b ? 1 : 0;
}

function compareKinds(a, b) {
  return kindRank(a) - kindRank(b) || compareText(a, b);
}

function clearKeyList() {
  keyGroups.clear();
  listedKeys.clear();
  keyListEl.innerHTML = "";
}

function keyGroup(kind) {
  let group = keyGroups.get(kind);
  if (group) return group;
  const el = document.createElement("div");
  el.className = "redis-group";
  el.classList.toggle("collapsed", foldedKinds.has(kind));
  const header = document.createElement("div");
  header.className = "redis-group-header";
  const toggle = document.createElement("span");
  toggle.className = "toggle";
  header.appendChild(toggle);
  const name = document.createElement("span");
  name.textContent = kind;
  header.appendChild(name);
  const count = document.createElement("span");
  count.className = "group-count";
  header.appendChild(count);
  header.addEventListener("click", () => {
    const folded = !foldedKinds.has(kind);
    if (folded) foldedKinds.add(kind);
    else foldedKinds.delete(kind);
    el.classList.toggle("collapsed", folded);
  });
  el.appendChild(header);
  const body = document.createElement("div");
  body.className = "redis-group-keys";
  el.appendChild(body);
  group = { el, count, body, entries: [] };
  // In order among the groups already shown: before the first that sorts after it.
  const next = [...keyGroups.keys()].sort(compareKinds).find((k) => compareKinds(kind, k) < 0);
  keyListEl.insertBefore(el, next ? keyGroups.get(next).el : null);
  keyGroups.set(kind, group);
  return group;
}

function addKeys(entries) {
  const touched = new Set();
  for (const entry of entries) {
    const id = keyId(entry);
    if (listedKeys.has(id)) continue;
    listedKeys.add(id);
    const group = keyGroup(entry.kind);
    group.entries.push(entry);
    touched.add(group);
  }
  for (const group of touched) {
    group.entries.sort((a, b) => compareText(a.key, b.key));
    // The items already shown are moved, not made again (`keyItem` keeps each one).
    group.body.replaceChildren(...group.entries.map(keyItem));
  }
  // "+": more keys may come with "Load more".
  const more = scanCursor !== "0" ? "+" : "";
  for (const group of keyGroups.values()) {
    group.count.textContent = "(" + group.entries.length + more + ")";
  }
}

// Each key's item, made once: "Load more" puts the group in order again, and remaking
// every item each time would grow with the square of the keys.
const keyItems = new WeakMap();

function keyItem(entry) {
  let item = keyItems.get(entry);
  if (item) return item;
  item = document.createElement("div");
  keyItems.set(entry, item);
  item.className = "table-list-item";
  item.title = entry.key;
  const name = document.createElement("span");
  name.textContent = entry.key;
  item.appendChild(name);
  item.addEventListener("click", () => openKey(entry));
  return item;
}

// ── A key's tab ──

// `entry` is a key from the list: `key` is its name to show, and `raw` its bytes (hex)
// when the name is binary (not valid UTF-8, or with a control character). Rust opens the
// key by `raw` when there is one.
async function openKey(entry) {
  const tab = addTab(keyId(entry), entry.key, "database", true);
  await renderKey(entry, tab.pane);
}

// The latest render of each pane, so a reply that an earlier render (a double click, a
// second press of reload) is still waiting for does not draw the view twice.
const paneRenders = new WeakMap();

async function renderKey(entry, pane) {
  const key = entry.key;
  const gen = generation;
  const render = (paneRenders.get(pane) || 0) + 1;
  paneRenders.set(pane, render);
  const stale = () => gen !== generation || paneRenders.get(pane) !== render;
  pane.innerHTML = "";
  let view;
  try {
    view = await safeInvoke("redis_key", { key, raw: entry.raw ?? null });
  } catch (error) {
    if (stale()) return;
    pane.textContent = String(error);
    return;
  }
  if (stale()) return;

  const header = document.createElement("div");
  header.className = "redis-key-header";
  const factsEl = document.createElement("div");
  factsEl.className = "redis-facts";
  // One labelled item of the header: "TTL  3594 s (until ...)".
  const fact = (label, value) => {
    const item = document.createElement("span");
    item.className = "redis-fact";
    const labelEl = document.createElement("span");
    labelEl.className = "redis-fact-label";
    labelEl.textContent = label;
    item.appendChild(labelEl);
    item.appendChild(document.createTextNode(value));
    factsEl.appendChild(item);
  };
  if (view.kind === "none") {
    factsEl.textContent = t("redis_key_gone");
  } else {
    fact(t("redis_type"), view.kind);
    fact("TTL", view.ttl < 0
      ? t("redis_no_expiry")
      : t("redis_ttl_value", { n: view.ttl, at: new Date(Date.now() + view.ttl * 1000).toLocaleString() }));
    if (view.memory !== null) fact(t("redis_memory"), t("redis_bytes", { n: view.memory }));
    fact(view.text !== null ? t("redis_length") : t("redis_size"), String(view.size));
  }
  header.appendChild(factsEl);

  const actions = document.createElement("div");
  actions.className = "redis-key-actions";
  if (view.text !== null) {
    // A value cut at 1 MiB copies only what is shown, and the button says so.
    actions.appendChild(copyButton(view.text, view.truncated ? t("redis_copy_head") : t("copy")));
  } else if (view.columns.length > 0) {
    const exportBtn = document.createElement("button");
    exportBtn.className = "ghost";
    exportBtn.innerHTML = icon("download") + t("export") + " ▾";
    exportBtn.addEventListener("click", (ev) => {
      ev.stopPropagation();
      showContextMenu(ev, currentRowsExportItems(view.columns, view.rows, key));
    });
    actions.appendChild(exportBtn);
  }
  const reloadBtn = document.createElement("button");
  reloadBtn.className = "ghost sidebar-btn";
  reloadBtn.title = t("redis_refresh");
  reloadBtn.innerHTML = icon("rotate-cw");
  reloadBtn.addEventListener("click", () => renderKey(entry, pane));
  actions.appendChild(reloadBtn);
  header.appendChild(actions);
  pane.appendChild(header);

  if (view.truncated) {
    const note = document.createElement("div");
    note.className = "redis-note";
    note.textContent = view.text !== null
      ? t("redis_string_truncated")
      : t("redis_truncated", { shown: view.rows.length, total: view.size });
    pane.appendChild(note);
  }
  if (view.binary) {
    const note = document.createElement("div");
    note.className = "redis-note";
    note.textContent = t("redis_binary_value");
    pane.appendChild(note);
  }

  const body = document.createElement("div");
  body.className = "redis-key-body";
  pane.appendChild(body);
  if (view.text !== null) {
    const pre = document.createElement("pre");
    pre.className = "redis-value";
    pre.textContent = view.binary ? view.text : (prettyJson(view.text) ?? view.text);
    body.appendChild(pre);
  } else if (view.columns.length > 0) {
    renderPagedTable(view.columns, view.rows, body, showRowDetailModal);
  } else if (view.kind !== "none") {
    body.textContent = t("redis_unsupported_type", { kind: view.kind });
  }
}

// A button labelled `label` that copies `text` to the clipboard and says so for a moment.
function copyButton(text, label) {
  const btn = document.createElement("button");
  btn.className = "ghost";
  const idle = () => { btn.innerHTML = icon("copy") + label; };
  idle();
  btn.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(text);
      btn.textContent = t("export_copied");
    } catch (error) {
      btn.textContent = t("export_failed", { msg: String(error) });
    }
    setTimeout(idle, 1500);
  });
  return btn;
}

// ── The CLI tab ──

let cliOutput = null;
let cliInput = null;
let history = [];
let historyIndex = 0;

function historyKey() {
  return "musql:redis-history:" + profileId;
}

function loadHistory() {
  try {
    history = JSON.parse(localStorage.getItem(historyKey())) || [];
  } catch (_) {
    history = [];
  }
  historyIndex = history.length;
}

// Lines that may carry a password (AUTH, ACL SETUSER, HELLO ... AUTH, MIGRATE ... AUTH,
// CONFIG SET requirepass / masterauth). redis-cli leaves these out of its history, and
// passwords are kept only in the credential manager. Matched loosely, anywhere in the
// line, so quoting does not get past it; an innocent line that matches is only left out.
const SECRET_WORDS = /\b(auth|auth2|acl|hello|migrate|requirepass|masterauth|masteruser)\b/i;

function saveHistory(line) {
  if (SECRET_WORDS.test(line)) return;
  if (history[history.length - 1] !== line) history.push(line);
  if (history.length > 100) history = history.slice(-100);
  historyIndex = history.length;
  try {
    localStorage.setItem(historyKey(), JSON.stringify(history));
  } catch (_) {
    // Storage full or unavailable: the history stays for this session only.
  }
}

// ── Completion and help ──

// The server's commands (`redis_command_docs`): name, group, summary, syntax. Empty until
// they arrive, or when the server does not give them; completion then has nothing to offer
// and the help shows the names below without their syntax.
let commandDocs = [];
let docsByName = new Map();
const SUGGEST_LIMIT = 8;

// The help's list when nothing is typed: the commands most used per type.
const MAIN_COMMANDS = [
  ["keys", ["SCAN", "EXISTS", "TYPE", "TTL", "EXPIRE", "PERSIST", "DEL", "RENAME"]],
  ["string", ["GET", "SET", "MGET", "INCR", "APPEND", "STRLEN"]],
  ["hash", ["HGET", "HSET", "HGETALL", "HDEL", "HKEYS", "HLEN"]],
  ["list", ["LRANGE", "LPUSH", "RPUSH", "LPOP", "RPOP", "LLEN"]],
  ["set", ["SMEMBERS", "SADD", "SREM", "SISMEMBER", "SCARD"]],
  ["zset", ["ZRANGE", "ZADD", "ZREM", "ZSCORE", "ZCARD"]],
  ["stream", ["XRANGE", "XADD", "XLEN", "XINFO STREAM"]],
  ["server", ["INFO", "DBSIZE", "SELECT", "PING", "CLIENT LIST"]],
];

let cliSuggest = null;
let cliHelp = null;
let suggestions = [];
let suggestIndex = -1;

async function loadCommandDocs() {
  const gen = generation;
  let docs;
  try {
    docs = await safeInvoke("redis_command_docs");
  } catch (_) {
    // Completion is a convenience: without the docs the CLI still works.
    return;
  }
  if (gen !== generation) return;
  commandDocs = docs;
  docsByName = new Map(docs.map((d) => [d.name, d]));
  updateHelp(true);
}

// The command the line starts with. Its first two words are tried before the first, so
// `CONFIG GET maxmemory` finds CONFIG GET rather than CONFIG.
function docForLine(line) {
  const words = line.trim().toUpperCase().split(/\s+/);
  return docsByName.get(words.slice(0, 2).join(" ")) || docsByName.get(words[0]) || null;
}

// The line was edited (typed, or a command put in by a click): suggestions and help
// follow it.
function onLineEdited() {
  updateSuggestions();
  updateHelp();
}

function closeSuggestions() {
  suggestions = [];
  renderSuggestions();
}

function updateSuggestions() {
  const typed = cliInput.value.trimStart().toUpperCase();
  suggestions = typed
    ? commandDocs.filter((d) => d.name.startsWith(typed) && d.name !== typed.trimEnd()).slice(0, SUGGEST_LIMIT)
    : [];
  suggestIndex = -1;
  renderSuggestions();
}

function renderSuggestions() {
  cliSuggest.innerHTML = "";
  cliSuggest.classList.toggle("hidden", suggestions.length === 0);
  suggestions.forEach((doc, i) => {
    const item = document.createElement("div");
    item.className = "redis-suggest-item" + (i === suggestIndex ? " active" : "");
    const name = document.createElement("span");
    name.className = "redis-suggest-name";
    name.textContent = doc.name;
    item.appendChild(name);
    const syntax = document.createElement("span");
    syntax.className = "redis-suggest-syntax";
    syntax.textContent = doc.syntax;
    item.appendChild(syntax);
    // mousedown, not click: the input must keep the focus.
    item.addEventListener("mousedown", (e) => {
      e.preventDefault();
      acceptSuggestion(i);
    });
    cliSuggest.appendChild(item);
  });
}

function acceptSuggestion(i) {
  const doc = suggestions[i];
  if (!doc) return;
  cliInput.value = doc.name + " ";
  closeSuggestions();
  updateHelp();
}

// What the help shows now (a command, or null for the main commands), so a key press
// that does not change it does not rebuild it.
let helpShown;

// Below the input: the syntax and summary of the command being typed, or the main
// commands (click one to start a line with it) when nothing matches.
function updateHelp(force) {
  if (!cliHelp) return;
  const doc = cliInput.value.trim() ? docForLine(cliInput.value) : null;
  if (!force && doc === helpShown) return;
  helpShown = doc;
  cliHelp.innerHTML = "";
  if (doc) {
    const syntax = document.createElement("div");
    syntax.className = "redis-help-syntax";
    syntax.textContent = doc.name + (doc.syntax ? " " + doc.syntax : "");
    cliHelp.appendChild(syntax);
    if (doc.summary) {
      const summary = document.createElement("div");
      summary.className = "redis-help-summary";
      summary.textContent = (doc.group ? "[" + doc.group + "] " : "") + doc.summary;
      cliHelp.appendChild(summary);
    }
    return;
  }
  const hint = document.createElement("div");
  hint.className = "redis-help-summary";
  hint.textContent = t("redis_help_hint");
  cliHelp.appendChild(hint);
  for (const [group, names] of MAIN_COMMANDS) {
    const row = document.createElement("div");
    row.className = "redis-help-row";
    const label = document.createElement("span");
    label.className = "redis-help-group";
    label.textContent = group;
    row.appendChild(label);
    for (const name of names) {
      const chip = document.createElement("button");
      chip.className = "ghost redis-help-chip";
      chip.textContent = name;
      const known = docsByName.get(name);
      if (known) chip.title = name + " " + known.syntax + "\n" + known.summary;
      chip.addEventListener("click", () => {
        cliInput.value = name + " ";
        cliInput.focus();
        onLineEdited();
      });
      row.appendChild(chip);
    }
    cliHelp.appendChild(row);
  }
}

function buildCliTab() {
  const tab = addTab("cli", t("redis_cli"), "terminal", false);
  tab.pane.classList.add("redis-cli");
  cliOutput = document.createElement("pre");
  cliOutput.className = "redis-cli-output";
  tab.pane.appendChild(cliOutput);
  // The suggestions sit right above the input, over the end of the output.
  const inputArea = document.createElement("div");
  inputArea.className = "redis-cli-input-area";
  cliSuggest = document.createElement("div");
  cliSuggest.className = "redis-suggest hidden";
  inputArea.appendChild(cliSuggest);
  const row = document.createElement("div");
  row.className = "redis-cli-input";
  const prompt = document.createElement("span");
  prompt.textContent = ">";
  row.appendChild(prompt);
  cliInput = document.createElement("input");
  cliInput.placeholder = t("redis_cli_placeholder");
  cliInput.spellcheck = false;
  row.appendChild(cliInput);
  const aiBtn = document.createElement("button");
  aiBtn.className = "ghost";
  aiBtn.innerHTML = icon("sparkles") + " " + t("ai_assist");
  aiBtn.addEventListener("click", openAiAssist);
  row.appendChild(aiBtn);
  inputArea.appendChild(row);
  tab.pane.appendChild(inputArea);
  cliHelp = document.createElement("div");
  cliHelp.className = "redis-cli-help";
  tab.pane.appendChild(cliHelp);

  cliInput.addEventListener("input", onLineEdited);
  cliInput.addEventListener("blur", closeSuggestions);
  cliInput.addEventListener("keydown", (e) => {
    if (e.isComposing) return;
    // While suggestions are open, the arrows move among them and Tab / Enter take one.
    if (suggestions.length > 0) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const step = e.key === "ArrowDown" ? 1 : -1;
        suggestIndex = (suggestIndex + step + suggestions.length + 1) % (suggestions.length + 1);
        if (suggestIndex === suggestions.length) suggestIndex = -1;
        renderSuggestions();
        return;
      }
      if (e.key === "Tab" || (e.key === "Enter" && suggestIndex >= 0)) {
        e.preventDefault();
        acceptSuggestion(Math.max(suggestIndex, 0));
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        closeSuggestions();
        return;
      }
    }
    if (e.key === "Tab") {
      e.preventDefault(); // stay in the input
    } else if (e.key === "Enter") {
      e.preventDefault();
      runCliLine(cliInput.value);
    } else if (e.key === "ArrowUp" && historyIndex > 0) {
      e.preventDefault();
      historyIndex--;
      cliInput.value = history[historyIndex];
      updateHelp();
    } else if (e.key === "ArrowDown" && historyIndex < history.length) {
      e.preventDefault();
      historyIndex++;
      cliInput.value = history[historyIndex] || "";
      updateHelp();
    }
  });
  updateHelp(true);
}

// The AI assist (ai-assist.js): redis-cli commands from Rust (`redis_ai_assist`, which
// tells the AI the key names and types, not the values). An answer goes into the CLI
// input, one line at a time, and runs only when the user presses Enter.
function openAiAssist() {
  openAiAssistModal({
    chatKey: aiChatKey(profileId, currentDb),
    placeholder: t("redis_ai_placeholder"),
    emptyText: t("redis_ai_empty"),
    request: (prompt, conversationContext, ai) => safeInvoke("redis_ai_assist", {
      prompt,
      conversationContext,
      ...ai,
    }),
    insertLabel: t("redis_ai_insert"),
    insert: (text) => {
      activateTab("cli");
      cliInput.value = text;
      updateHelp();
      cliInput.focus();
    },
    perLine: true,
  });
}

function appendCli(text, className) {
  const line = document.createElement("div");
  if (className) line.className = className;
  line.textContent = text;
  cliOutput.appendChild(line);
  while (cliOutput.childElementCount > CLI_LINE_LIMIT) cliOutput.firstElementChild.remove();
  cliOutput.scrollTop = cliOutput.scrollHeight;
}

// The lines run one after another, in the order they were entered: each Rust call takes
// the connection's lock on its own, so two lines sent quickly could otherwise run in
// either order (SET a 1, then GET a).
let cliQueue = Promise.resolve();

function runCliLine(line) {
  if (!line.trim()) return;
  cliInput.value = "";
  closeSuggestions();
  updateHelp();
  saveHistory(line);
  appendCli("> " + line, "redis-cli-command");
  const gen = generation;
  cliQueue = cliQueue.then(() => runQueuedLine(line, gen));
}

// Which commands need a confirmation is Rust's call (it reads the command as the server
// will): a dangerous one comes back unrun with `confirm`, and is sent again once the user
// agreed.
async function runQueuedLine(line, gen) {
  if (gen !== generation) return;
  try {
    let result = await safeInvoke("redis_command", { line, confirmed: false });
    if (gen !== generation) return;
    if (result.confirm !== null) {
      if (!window.confirm(t("redis_confirm_danger", { command: result.confirm }))) {
        appendCli(t("redis_cancelled"), "redis-cli-error");
        return;
      }
      result = await safeInvoke("redis_command", { line, confirmed: true });
      if (gen !== generation) return;
    }
    appendCli(result.reply);
    applyDb(result.db);
  } catch (error) {
    if (gen !== generation) return;
    appendCli(String(error), "redis-cli-error");
  }
}

// ── Open and close ──

// A disconnect that reaches Rust after a newer connect leaves that connect alone (both
// are numbered: `redis_client::next_epoch`).
function disconnect() {
  return safeInvoke("redis_disconnect").catch(() => {});
}

// The window closed: drop the connection and clear the page.
function reset() {
  disconnect();
  clearView();
}

function clearView() {
  if (dockerTunnelContainerId) {
    safeInvoke("docker_stop_tunnel", { containerId: dockerTunnelContainerId }).catch(() => {});
    dockerTunnelContainerId = null;
  }
  generation++;
  listing++;
  cancelCredentialPrompt();
  hideContextMenu();
  closeAiAssistModal();
  hideAiModal();
  foldedKinds.clear();
  removeAllTabs();
  commandDocs = [];
  docsByName = new Map();
  cliHelp = null;
  clearKeyList();
  moreBtn.classList.add("hidden");
  patternInput.value = "";
  explorerEl.classList.add("hidden");
  showStatus("");
  profileId = null;
  profileName = "";
  updateWindowTitle();
}

async function open(id) {
  clearView();
  const gen = generation;
  // The earlier connection (and its SSH tunnel) goes now, even if this open stops at the
  // prompt; and it is gone before redis_connect starts.
  await disconnect();
  if (gen !== generation) return;
  const data = await safeInvoke("list_profiles");
  if (gen !== generation) return;
  const profile = data.items.find((item) => item.id === id);
  if (!profile) {
    showStatus("Profile not found");
    return;
  }
  profileId = id;
  profileName = profile.name;
  const request = profile.request;
  const answered = await promptForCredentials(request);
  // The window may have been closed (or another profile opened) while the prompt was up.
  if (gen !== generation) return;
  if (!answered) {
    showStatus(t("redis_not_connected"));
    return;
  }
  await connect(request, id, gen);
}

// A running Docker container (#98): the main window's Docker list resolved the address
// (a published port, or a socat tunnel) and the credentials, so there is no profile and
// no prompt. The tunnel is stopped when the window closes.
async function openDocker(info) {
  clearView();
  // Recorded at once: a close (or another open) while the disconnect below is on its
  // way must still stop this tunnel (clearView does).
  dockerTunnelContainerId = info.tunnel_container_id || null;
  const gen = generation;
  await disconnect();
  if (gen !== generation) return;
  profileId = "docker:" + info.name;
  profileName = info.name;
  const request = {
    engine: "redis",
    mysql: {
      host: info.host,
      port: info.port,
      database: null,
      username: info.user || "",
      password: info.password || "",
      ssl_mode: info.ssl_mode || "DISABLED",
      tls_ca_cert_path: null,
      save_password: true,
    },
    ssh: null,
  };
  // No profile id: the credentials are in the request, and there is no profile entry in
  // the credential manager to look up.
  await connect(request, null, gen);
}

async function connect(request, id, gen) {
  showStatus(t("redis_connecting"));
  try {
    const server = await safeInvoke("redis_connect", { request, profileId: id });
    if (gen !== generation) return;
    serverVersion = server.version;
    currentDb = server.db;
  } catch (error) {
    if (gen === generation) showStatus(String(error));
    return;
  }
  showStatus("");
  explorerEl.classList.remove("hidden");
  updateServerLabel();
  updateWindowTitle();
  loadHistory();
  buildCliTab();
  loadKeys(true);
  loadCommandDocs();
}

// ── Event wiring ──

refreshBtn.addEventListener("click", () => loadKeys(true));
moreBtn.addEventListener("click", () => loadKeys(false));
serverEl.addEventListener("click", (e) => {
  e.stopPropagation();
  openDbMenu();
});
// The list follows the pattern box as it is typed (after a short pause, so each key
// press does not start a scan); Enter searches at once.
let patternTimer = null;
patternInput.addEventListener("input", () => {
  clearTimeout(patternTimer);
  patternTimer = setTimeout(() => loadKeys(true), 300);
});
patternInput.addEventListener("keydown", (e) => {
  if (e.key !== "Enter" || e.isComposing) return;
  clearTimeout(patternTimer);
  loadKeys(true);
});

// Note: unlisten is not needed — windows use show/hide pattern, so listeners register once.
if (eventApi && eventApi.listen) {
  eventApi.listen("redis:open", (event) => {
    open(String(event.payload || "")).catch((error) => showStatus(String(error)));
  });
  eventApi.listen("redis:docker-open", (event) => {
    if (event.payload) openDocker(event.payload).catch((error) => showStatus(String(error)));
  });
  eventApi.listen("redis:reset", () => reset());
}
// The menu's items for this window only (the global listen would also see the other
// windows' items).
if (currentWindow) {
  currentWindow.listen("menu:action", (event) => {
    switch (event.payload) {
      case "theme-light": setTheme("light"); break;
      case "theme-dark": setTheme("dark"); break;
      case "lang-en": setLang("en"); break;
      case "lang-ja": setLang("ja"); break;
      case "ai-settings": showAiModal(); break;
    }
  });
}

window.addEventListener("musql:langchange", () => {
  applyI18n();
  if (profileId) updateServerLabel();
  updateHelp(true);
});

applyI18n();
