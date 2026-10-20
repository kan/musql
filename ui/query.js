const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;
const eventApi = window.__TAURI__ && window.__TAURI__.event ? window.__TAURI__.event : null;

// ── DOM refs ──
const dbModal = document.getElementById("db-modal");
const dbList = document.getElementById("db-list");
const dbModalStatus = document.getElementById("db-modal-status");
const explorerEl = document.getElementById("explorer");
const sidebarDbName = document.getElementById("sidebar-db-name");
const dbSwitchBtn = document.getElementById("db-switch-btn");
const tableListEl = document.getElementById("table-list");
const tabBarTabs = document.getElementById("tab-bar-tabs");
const tabContent = document.getElementById("tab-content");
const tabAddBtn = document.getElementById("tab-add-btn");

const menuBtn = document.getElementById("menu-btn");

// The AI settings and the AI assist chat are in ai-assist.js, shared with the Redis
// window (#98).

// Apply icons to static elements
menuBtn.innerHTML = icon('menu');
dbSwitchBtn.innerHTML = icon('arrow-left-right');
tabAddBtn.innerHTML = icon('plus');

menuBtn.addEventListener("click", () => safeInvoke("show_popup_menu", { lang: getLang(), theme: getTheme(), notify: isNotifyEnabled() }));
document.getElementById("db-modal-heading").innerHTML = icon('database', 20) + ' ' + t('select_database');

// ── State ──
let requestCache = null;
let currentDb = null;
let currentTables = [];
let currentProfileId = null;
let currentProfileName = null;
let dockerTunnelContainerId = null;
const sqlTabActions = {}; // tabId → { runLine, runAll, format, history, cancel }
const closedDrafts = {}; // tabNumber → content (in-session restore for closed tabs)

// ── Draft save/restore ──

let draftSaveTimer = null;

function saveDrafts() {
  if (!currentProfileId) return;
  const tabs = [];
  tabManager.tabs.forEach((tab) => {
    if (tab.type === "sql") {
      const cmEl = tab.paneEl.querySelector(".CodeMirror");
      const content = (cmEl && cmEl.CodeMirror) ? cmEl.CodeMirror.getValue() : "";
      const match = tab.id.match(/^sql-(\d+)$/);
      const num = match ? parseInt(match[1], 10) : 0;
      tabs.push({ type: "sql", num, content });
    } else if (tab.type === "data") {
      const match = tab.id.match(/^table-(.+)$/);
      if (match) {
        const view = tab.paneEl.dataset.view || "data";
        tabs.push({ type: "table", name: match[1], view });
      }
    }
  });
  const key = "musql:drafts:" + currentProfileId;
  const hasContent = tabs.some((t) => t.type === "table" || (t.type === "sql" && t.content));
  if (tabs.length === 0 || !hasContent) {
    localStorage.removeItem(key);
  } else {
    localStorage.setItem(key, JSON.stringify({ tabs, activeTabId: tabManager.activeId || null }));
  }
}

function loadDrafts() {
  if (!currentProfileId) return { tabs: [], activeTabId: null };
  try {
    const raw = JSON.parse(localStorage.getItem("musql:drafts:" + currentProfileId));
    if (!raw) return { tabs: [], activeTabId: null };

    // New format: { tabs: [...], activeTabId }
    if (raw.tabs && Array.isArray(raw.tabs)) {
      return { tabs: raw.tabs, activeTabId: raw.activeTabId || null };
    }

    // Old format: array of SQL-only drafts
    if (Array.isArray(raw)) {
      const sqlTabs = raw.map((d, i) => {
        if (typeof d === "string") return { type: "sql", num: i + 1, content: d };
        if (d.num !== undefined) return { type: "sql", num: d.num, content: d.content || "" };
        return { type: "sql", num: i + 1, content: d.content || "" };
      });
      return { tabs: sqlTabs, activeTabId: null };
    }

    return { tabs: [], activeTabId: null };
  } catch (_) { return { tabs: [], activeTabId: null }; }
}

function scheduleDraftSave() {
  if (draftSaveTimer) clearTimeout(draftSaveTimer);
  draftSaveTimer = setTimeout(saveDrafts, 1000);
}

// ── Execution history ──

const HISTORY_MAX = 100;

function saveHistory(sql) {
  if (!currentProfileId) return;
  const key = "musql:history:" + currentProfileId;
  let history = [];
  try { history = JSON.parse(localStorage.getItem(key)) || []; } catch (_) {}
  if (history.length > 0 && history[0].sql === sql) return;
  history.unshift({ sql, ts: Date.now() });
  if (history.length > HISTORY_MAX) history = history.slice(0, HISTORY_MAX);
  localStorage.setItem(key, JSON.stringify(history));
}

function loadHistory() {
  if (!currentProfileId) return [];
  try { return JSON.parse(localStorage.getItem("musql:history:" + currentProfileId)) || []; }
  catch (_) { return []; }
}

function showHistoryMenu(anchorEvent, entries, onSelect) {
  // Remove any existing history menu
  const existing = document.querySelector(".history-menu-container");
  if (existing) existing.remove();

  const container = document.createElement("div");
  container.className = "history-menu-container";

  const list = document.createElement("div");
  list.className = "history-menu-list";

  const preview = document.createElement("div");
  preview.className = "history-menu-preview";
  const previewPre = document.createElement("pre");
  preview.appendChild(previewPre);

  entries.forEach((entry, i) => {
    const d = new Date(entry.ts);
    const time = String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
    const sqlPreview = entry.sql.replace(/\s+/g, " ");
    const label = time + "  " + (sqlPreview.length > 60 ? sqlPreview.substring(0, 60) + "\u2026" : sqlPreview);

    const el = document.createElement("div");
    el.className = "history-menu-item";
    el.textContent = label;
    el.addEventListener("mouseenter", () => {
      list.querySelectorAll(".history-menu-item").forEach((x) => x.classList.remove("active"));
      el.classList.add("active");
      previewPre.textContent = entry.sql;
      preview.classList.add("visible");
    });
    el.addEventListener("click", () => {
      close();
      onSelect(entry.sql);
    });
    list.appendChild(el);

    // Show first item preview by default
    if (i === 0) {
      el.classList.add("active");
      previewPre.textContent = entry.sql;
      preview.classList.add("visible");
    }
  });

  container.appendChild(list);
  container.appendChild(preview);
  document.body.appendChild(container);

  // Position relative to click
  const x = anchorEvent.clientX;
  const y = anchorEvent.clientY;
  container.style.left = x + "px";
  container.style.top = y + "px";

  // Adjust if overflowing
  requestAnimationFrame(() => {
    const rect = container.getBoundingClientRect();
    if (rect.right > window.innerWidth) {
      container.style.left = Math.max(4, window.innerWidth - rect.width - 4) + "px";
    }
    if (rect.bottom > window.innerHeight) {
      container.style.top = Math.max(4, window.innerHeight - rect.height - 4) + "px";
    }
  });

  function close() {
    container.remove();
    document.removeEventListener("click", onDocClick, true);
    document.removeEventListener("keydown", onKey);
  }
  function onDocClick(e) {
    if (!container.contains(e.target)) close();
  }
  function onKey(e) { if (e.key === "Escape") close(); }
  setTimeout(() => {
    document.addEventListener("click", onDocClick, true);
    document.addEventListener("keydown", onKey);
  }, 0);
}

function buildTableHints() {
  const hints = {};
  currentTables.forEach((t) => { hints[t] = []; });
  return hints;
}

// ── Helpers ──

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

// The credential prompt (`promptForCredentials`) lives in credential-prompt.js, shared
// with the Redis window (#98).

async function loadProfile(id) {
  const data = await safeInvoke("list_profiles");
  const profile = data.items.find((item) => item.id === id);
  if (!profile) throw new Error("profile not found");
  requestCache = profile.request;
  currentProfileName = profile.name;
}

function runQuery(sql, maxRows, tabId) {
  const payload = { request: requestCache, query: sql, profileId: currentProfileId || null };
  if (maxRows != null) payload.maxRows = maxRows;
  if (tabId) payload.tabId = tabId;
  return safeInvoke("run_query", payload);
}

// ── Long-running query completion notification (#43) ──
// Notify (desktop toast) when a query that ran longer than the threshold finishes
// while the window is unfocused. Three ways are tried in order (maybeNotifyQueryDone):
// a clickable toast shown by Rust, then the plugin, then the Web Notification.
const NOTIFY_THRESHOLD_MS = 5000;

// Counts the tab sets of this window. Closing the window or switching database drops
// every tab (`tabManager.removeAll`), and the next set reuses the same ids ("sql-1").
// A toast from an earlier set must not activate the new tab of the same name, so its
// token carries the number the query *started* under.
let notifySession = 0;
// Set once Rust has said it cannot show a clickable toast (the Store build), so the
// call is not repeated for every notification.
let clickableToastUnavailable = false;

function isNotifyEnabled() {
  return localStorage.getItem("musql:notify-query") !== "0"; // default ON
}
function setNotifyEnabled(on) {
  localStorage.setItem("musql:notify-query", on ? "1" : "0");
}

let _notifier; // NotifyFn | null | undefined (resolved once, then cached)
function webNotifier() {
  return (title, body) => { new Notification(title, { body }); };
}
// tauri-plugin-notification first: it uses the app's identity, so the title is correct on
// the installed app (WebView2's Web Notification instead shows the launching process —
// "powershell" in dev; installed builds show muSQL). Web Notification is only a fallback.
// Neither can be clicked through: WebView2 does not deliver Web Notification `onclick`,
// and notify_rust has no click callback on Windows desktop. These two are what is left
// when the clickable toast (`toast_notify`) is not available.
async function resolveNotifier() {
  if (_notifier) return _notifier; // only a successful notifier is cached; retry after failure
  const resolved = await (async () => {
    const np = window.__TAURI__ && window.__TAURI__.notification;
    if (np) {
      try {
        let granted = await np.isPermissionGranted();
        if (!granted) granted = (await np.requestPermission()) === "granted";
        if (granted) return (title, body) => { np.sendNotification({ title, body }); };
      } catch (_) { /* ignore */ }
    }
    if ("Notification" in window) {
      if (Notification.permission === "granted") return webNotifier();
      if (Notification.permission !== "denied") {
        try { if ((await Notification.requestPermission()) === "granted") return webNotifier(); }
        catch (_) { /* ignore */ }
      }
    }
    return null;
  })();
  if (resolved) _notifier = resolved;
  return resolved;
}

// `session` is `notifySession` as it was when the query started.
async function maybeNotifyQueryDone(elapsedMs, body, tabId, session) {
  if (!isNotifyEnabled() || elapsedMs < NOTIFY_THRESHOLD_MS) return;
  // The tab that ran the query is gone (window closed, database switched). Its result
  // has nowhere to be shown, and the title below would name the wrong database.
  if (session !== notifySession) return;
  if (document.hasFocus()) return; // user is already looking at the window
  const title = sidebarDbName.textContent || "muSQL";
  // A toast that can be clicked (#113): Rust shows it, and when it is clicked sends the
  // token back in `toast:activated`. It answers false on the Store build, which cannot
  // do that; a rejection means it should have worked. Either way the plugin below
  // still shows a plain toast.
  if (!clickableToastUnavailable) {
    try {
      const token = session + ":" + tabId;
      if (await safeInvoke("toast_notify", { token, title, body })) return;
      clickableToastUnavailable = true;
    } catch (e) {
      console.warn("toast_notify failed:", e);
    }
  }
  const notify = await resolveNotifier();
  if (!notify) return;
  // Re-check focus: resolving may have shown a permission prompt that the user
  // dismissed by refocusing the window.
  if (document.hasFocus()) return;
  try { notify(title, body); }
  catch (_) { /* notification backend unavailable */ }
}

function setDatabase(dbName) {
  currentDb = dbName;
  if (requestCache && requestCache.mysql) {
    requestCache.mysql.database = dbName;
  }
  updateWindowTitle();
}

function updateWindowTitle() {
  const parts = [];
  if (currentProfileName) parts.push(currentProfileName);
  if (currentDb) parts.push(currentDb);
  const title = parts.length > 0 ? parts.join(" / ") + " — muSQL" : "muSQL Query";
  // Through Rust, not the JS setTitle: the dev build marker is added there.
  safeInvoke("set_window_title", { title }).catch((e) => console.warn("set_window_title failed:", e));
}

// ── Export utilities (CSV and the options dialog are in export.js) ──

// ── SQL dialect ──
// The window speaks MySQL's SQL, and SQLite's for a SQLite profile (#97). Every piece of
// SQL that differs between the two is built by the functions in this section.

function isSqlite() {
  return !!requestCache && requestCache.engine === "sqlite";
}

// The dialect's name for CodeMirror's mode (`text/x-…`) and sql-formatter's language.
function sqlDialect() {
  return isSqlite() ? "sqlite" : "mysql";
}

function quoteId(name) {
  if (isSqlite()) return '"' + name.replace(/"/g, '""') + '"';
  return '`' + name.replace(/`/g, '``') + '`';
}

// A value as a SQL string literal. Every literal built in this file goes through here.
//
// The quote is doubled: `''` is one quote in either sql_mode, so nothing can end the
// literal early. On MySQL the backslash is doubled too, because MySQL treats it as an
// escape by default: left alone, a value ending in `\` would swallow the closing quote
// (#123). Under NO_BACKSLASH_ESCAPES a doubled backslash is two characters, so a value
// containing one fails to match; that is a miss, never an injection. `\'` is not used:
// under that mode it would close the literal. SQLite has no backslash escapes at all.
function sqlStringLiteral(val) {
  let s = String(val);
  if (!isSqlite()) s = s.replace(/\\/g, "\\\\");
  return "'" + s.replace(/'/g, "''") + "'";
}

// SQLite's own tables (sqlite_sequence, sqlite_stat1, ...) are left out of the lists.
const SQLITE_USER_OBJECTS = "name NOT LIKE 'sqlite\\_%' ESCAPE '\\'";

function tableListSql() {
  if (isSqlite()) {
    return "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND " + SQLITE_USER_OBJECTS + " ORDER BY name";
  }
  return "SHOW TABLES";
}

// The CREATE statement of a table (or view), or "" when it cannot be read.
async function fetchCreateSql(tableName) {
  try {
    if (isSqlite()) {
      const res = await runQuery("SELECT sql FROM sqlite_master WHERE type IN ('table', 'view') AND name = " + sqlStringLiteral(tableName));
      return res.rows.length > 0 ? (res.rows[0][0] || "") : "";
    }
    const res = await runQuery("SHOW CREATE TABLE " + quoteId(tableName));
    return res.rows.length > 0 ? (res.rows[0][1] || "") : "";
  } catch (_) {
    return "";
  }
}

// The columns of a table, in order: [{ name, dataType (lower case), columnKey ("PRI" for
// the primary key) }].
async function fetchColumnMeta(tableName) {
  if (isSqlite()) {
    const res = await runQuery("SELECT name, type, pk FROM pragma_table_info(" + sqlStringLiteral(tableName) + ") ORDER BY cid");
    return res.rows.map((r) => ({ name: r[0], dataType: (r[1] || "").toLowerCase(), columnKey: Number(r[2]) > 0 ? "PRI" : "" }));
  }
  const res = await runQuery(
    "SELECT COLUMN_NAME, DATA_TYPE, COLUMN_KEY " +
    "FROM INFORMATION_SCHEMA.COLUMNS " +
    "WHERE TABLE_SCHEMA = " + sqlStringLiteral(currentDb) + " " +
    "AND TABLE_NAME = " + sqlStringLiteral(tableName) + " " +
    "ORDER BY ORDINAL_POSITION"
  );
  return res.rows.map((r) => ({ name: r[0], dataType: (r[1] || "").toLowerCase(), columnKey: r[2] || "" }));
}

const MYSQL_BLOB_TYPES = ["blob", "tinyblob", "mediumblob", "longblob", "binary", "varbinary"];
const MYSQL_TEXT_TYPES = ["text", "tinytext", "mediumtext", "longtext"];

// "blob" for a column the Data view hides, "text" for one it cuts to 200 characters,
// else null. SQLite's declared types are free text, so they are matched by the words
// SQLite itself reads them by (a declared BLOB, or TEXT / CLOB).
function columnKind(dataType) {
  if (isSqlite()) {
    if (dataType.includes("blob")) return "blob";
    if (dataType.includes("text") || dataType.includes("clob")) return "text";
    return null;
  }
  if (MYSQL_BLOB_TYPES.includes(dataType)) return "blob";
  if (MYSQL_TEXT_TYPES.includes(dataType)) return "text";
  return null;
}

// A text column cut to its first 200 characters, keeping its name.
function truncatedTextExpr(q) {
  if (isSqlite()) {
    return "CASE WHEN length(" + q + ") > 200 THEN substr(" + q + ", 1, 200) || '…' ELSE " + q + " END AS " + q;
  }
  return "CASE WHEN CHAR_LENGTH(" + q + ") > 200 THEN CONCAT(LEFT(" + q + ", 200), '…') ELSE " + q + " END AS " + q;
}

// A SQLite table's indexes, one row per column: index_name, unique, origin, partial,
// seqno, column_name.
function sqliteIndexSql(tableName) {
  return "SELECT il.name AS index_name, il.\"unique\", il.origin, il.partial, ii.seqno, ii.name AS column_name " +
    "FROM pragma_index_list(" + sqlStringLiteral(tableName) + ") AS il, pragma_index_info(il.name) AS ii ORDER BY il.name, ii.seqno";
}

// The Schema view's two queries: the columns, and the indexes (one row per column).
function structureSql(tableName) {
  if (isSqlite()) return ["PRAGMA table_info(" + sqlStringLiteral(tableName) + ")", sqliteIndexSql(tableName)];
  return ["DESCRIBE " + quoteId(tableName), "SHOW INDEX FROM " + quoteId(tableName)];
}

// Where a syntax error points: { near, line (0-based) }, or null. MySQL says
// "... near 'xxx' at line N". SQLite's message names no line, and searching the whole
// editor for the word could mark it in another statement, so it is not marked.
function sqlErrorPosition(errorMsg) {
  if (isSqlite()) return null;
  const m = /near\s+'([\s\S]*?)'\s+at\s+line\s+(\d+)/i.exec(errorMsg);
  return m ? { near: m[1], line: parseInt(m[2], 10) - 1 } : null;
}

// The first line of the SQL export.
function sqlDumpHeader() {
  return isSqlite() ? "-- muSQL export (SQLite)" : "-- mysqldump-compatible export";
}

function sqlEscapeValue(val) {
  if (val === null || val === undefined) return "NULL";
  if (typeof val === "number") return String(val);
  return sqlStringLiteral(val);
}

async function generateInsertSql(tableName, newline) {
  const createSql = await fetchCreateSql(tableName);

  const dataRes = await runQuery("SELECT * FROM " + quoteId(tableName), 0);
  const columns = dataRes.columns;
  const rows = dataRes.rows;

  const lines = [];
  lines.push(sqlDumpHeader());
  lines.push("-- Table: " + tableName);
  lines.push("-- Exported: " + new Date().toISOString());
  lines.push("");
  lines.push("DROP TABLE IF EXISTS " + quoteId(tableName) + ";");
  lines.push("");
  if (createSql) {
    // Split the multi-line CREATE TABLE (structural text) so the chosen line
    // ending applies to it too — avoids a mixed-EOL file (#40 review).
    (createSql + ";").split(/\r?\n/).forEach((l) => lines.push(l));
    lines.push("");
  }
  if (rows.length > 0) {
    const colList = columns.map((c) => quoteId(c)).join(", ");
    for (let i = 0; i < rows.length; i += 1000) {
      const batch = rows.slice(i, i + 1000);
      lines.push("INSERT INTO " + quoteId(tableName) + " (" + colList + ") VALUES");
      batch.forEach((row, j) => {
        const vals = columns.map((_, ci) => sqlEscapeValue(row[ci])).join(", ");
        lines.push("(" + vals + ")" + (j === batch.length - 1 ? ";" : ","));
      });
      lines.push("");
    }
  }
  // Record separator only; multi-line values inside SQL literals stay verbatim (#40).
  return lines.join(newline || "\n");
}

// The pieces of a table's schema that the Markdown export writes, gathered per dialect
// (`fetchMysqlSchemaParts`, `fetchSqliteSchemaParts`) and written by
// `renderMarkdownSchema`:
// - createSql, tableComment: strings, "" when there is none
// - columns: [{ name, type, defaultStr, nullable ("true" / "false"), extra, comment }]
// - fkParents, fkChildren: column -> [{ table, column, constraint }]
// - fkConstraints: constraint name -> { columns, refTable, refColumns }
// - indexes: [{ name, primary, unique, columns, using (null when unknown) }], in order
// - primaryKey: the primary key's columns when no index holds it (a SQLite table whose
//   key is its rowid), else null
async function generateMarkdownSchema(tableName, newline) {
  const parts = isSqlite() ? await fetchSqliteSchemaParts(tableName) : await fetchMysqlSchemaParts(tableName);
  return renderMarkdownSchema(tableName, parts, newline);
}

async function fetchSqliteSchemaParts(tableName) {
  const lit = sqlStringLiteral(tableName);
  const [createSql, infoRes, indexRes, fkRes, childRes] = await Promise.all([
    fetchCreateSql(tableName),
    runQuery("SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info(" + lit + ") ORDER BY cid"),
    runQuery(sqliteIndexSql(tableName)),
    runQuery("SELECT id, \"table\", \"from\", \"to\" FROM pragma_foreign_key_list(" + lit + ") ORDER BY id, seq"),
    // Table names compare without case in SQLite, as in a foreign key's REFERENCES.
    runQuery(
      "SELECT m.name, fk.\"from\", fk.\"to\", fk.id FROM sqlite_master AS m, pragma_foreign_key_list(m.name) AS fk " +
      "WHERE m.type = 'table' AND fk.\"table\" = " + lit + " COLLATE NOCASE"
    ),
  ]);

  const columns = infoRes.rows.map((r) => ({
    name: r[0],
    type: r[1] || "",
    defaultStr: r[3] == null ? "" : String(r[3]),
    nullable: Number(r[2]) ? "false" : "true",
    extra: "",
    comment: "",
  }));
  const primaryKey = infoRes.rows
    .filter((r) => Number(r[4]) > 0)
    .sort((a, b) => Number(a[4]) - Number(b[4]))
    .map((r) => r[0]);

  // SQLite's foreign keys have no names; tbls calls them by their id, as here.
  const fkName = (id) => "- (Foreign key ID: " + id + ")";
  const fkParents = {};
  const fkConstraints = {};
  for (const row of fkRes.rows) {
    const name = fkName(row[0]);
    const refTable = row[1];
    const from = row[2];
    // No "to": the parent's primary key, whose name this table does not record.
    const to = row[3] || "";
    if (!fkParents[from]) fkParents[from] = [];
    fkParents[from].push({ table: refTable, column: to, constraint: name });
    if (!fkConstraints[name]) fkConstraints[name] = { columns: [], refTable, refColumns: [] };
    fkConstraints[name].columns.push(from);
    fkConstraints[name].refColumns.push(to);
  }
  const fkChildren = {};
  for (const row of childRes.rows) {
    // No "to": the child points at this table's primary key.
    const col = row[2] || primaryKey[0];
    if (!col) continue;
    if (!fkChildren[col]) fkChildren[col] = [];
    fkChildren[col].push({ table: row[0], column: row[1], constraint: fkName(row[3]) });
  }

  // indexRes: index_name, unique, origin, partial, seqno, column_name (one row per column).
  const indexes = [];
  const byName = {};
  for (const row of indexRes.rows) {
    const name = row[0];
    if (!byName[name]) {
      // origin "pk": the index SQLite made for a PRIMARY KEY that is not the rowid.
      byName[name] = { name, primary: row[2] === "pk", unique: Number(row[1]) === 1, columns: [], using: null };
      indexes.push(byName[name]);
    }
    byName[name].columns.push(row[5] == null ? "(expression)" : row[5]);
  }

  return {
    createSql,
    tableComment: "",
    columns,
    fkParents,
    fkChildren,
    fkConstraints,
    indexes,
    primaryKey: primaryKey.length > 0 && !indexes.some((i) => i.primary) ? primaryKey : null,
  };
}

async function fetchMysqlSchemaParts(tableName) {
  const tableLiteral = sqlStringLiteral(tableName);
  const [createSql, descRes, indexRes, commentRes, colCommentRes, fkParentRes, fkChildRes] = await Promise.all([
    fetchCreateSql(tableName),
    runQuery("DESCRIBE " + quoteId(tableName)),
    runQuery("SHOW INDEX FROM " + quoteId(tableName)),
    runQuery("SELECT TABLE_COMMENT FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = " + tableLiteral),
    runQuery("SELECT COLUMN_NAME, COLUMN_COMMENT FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = " + tableLiteral + " ORDER BY ORDINAL_POSITION"),
    runQuery("SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = " + tableLiteral + " AND REFERENCED_TABLE_NAME IS NOT NULL"),
    runQuery("SELECT TABLE_NAME, COLUMN_NAME, CONSTRAINT_NAME FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = DATABASE() AND REFERENCED_TABLE_NAME = " + tableLiteral),
  ]);

  // Build lookup maps
  const colComments = {};
  if (colCommentRes && colCommentRes.rows) {
    for (const row of colCommentRes.rows) colComments[row[0]] = row[1] || "";
  }

  // FK parents: column -> [{table, column, constraint}]
  const fkParents = {};
  if (fkParentRes && fkParentRes.rows) {
    for (const row of fkParentRes.rows) {
      const col = row[1];
      if (!fkParents[col]) fkParents[col] = [];
      fkParents[col].push({ table: row[2], column: row[3], constraint: row[0] });
    }
  }

  // FK children: column -> [{table, column, constraint}]
  const fkChildren = {};
  if (fkChildRes && fkChildRes.rows) {
    for (const row of fkChildRes.rows) {
      const col = row[1];
      if (!fkChildren[col]) fkChildren[col] = [];
      fkChildren[col].push({ table: row[0], column: col, constraint: row[2] });
    }
  }

  // Index grouping: keyName -> { nonUnique, indexType, columns: [{col, seqInIndex}] }
  const indexMap = {};
  if (indexRes && indexRes.rows) {
    const ci = {};
    for (let i = 0; i < indexRes.columns.length; i++) ci[indexRes.columns[i]] = i;
    for (const row of indexRes.rows) {
      const keyName = row[ci["Key_name"]];
      const nonUnique = Number(row[ci["Non_unique"]]);
      const seqInIndex = Number(row[ci["Seq_in_index"]]);
      const colName = row[ci["Column_name"]];
      const indexType = row[ci["Index_type"]] || "BTREE";
      if (!indexMap[keyName]) indexMap[keyName] = { nonUnique, indexType, columns: [] };
      indexMap[keyName].columns.push({ col: colName, seq: seqInIndex });
    }
    for (const key of Object.keys(indexMap)) {
      indexMap[key].columns.sort((a, b) => a.seq - b.seq);
    }
  }
  const indexes = Object.entries(indexMap).map(([name, idx]) => ({
    name,
    primary: name === "PRIMARY",
    unique: idx.nonUnique === 0,
    columns: idx.columns.map((c) => c.col),
    using: idx.indexType,
  }));

  // FK constraint map: constraintName -> { columns, refTable, refColumns }
  const fkConstraints = {};
  if (fkParentRes && fkParentRes.rows) {
    for (const row of fkParentRes.rows) {
      const name = row[0];
      if (!fkConstraints[name]) fkConstraints[name] = { columns: [], refTable: row[2], refColumns: [] };
      fkConstraints[name].columns.push(row[1]);
      fkConstraints[name].refColumns.push(row[3]);
    }
  }

  const columns = [];
  if (descRes && descRes.rows) {
    const ci = {};
    for (let i = 0; i < descRes.columns.length; i++) ci[descRes.columns[i]] = i;
    for (const row of descRes.rows) {
      const field = row[ci["Field"]] || "";
      const defVal = row[ci["Default"]];
      columns.push({
        name: field,
        type: row[ci["Type"]] || "",
        defaultStr: (defVal === null || defVal === undefined) ? "" : String(defVal),
        nullable: (row[ci["Null"]] === "YES") ? "true" : "false",
        extra: row[ci["Extra"]] || "",
        comment: colComments[field] || "",
      });
    }
  }

  return {
    createSql,
    tableComment: (commentRes && commentRes.rows && commentRes.rows.length > 0) ? (commentRes.rows[0][0] || "") : "",
    columns,
    fkParents,
    fkChildren,
    fkConstraints,
    indexes,
    primaryKey: null,
  };
}

function renderMarkdownSchema(tableName, parts, newline) {
  const lines = [];

  // Title
  lines.push("# " + tableName);
  lines.push("");

  // Description
  if (parts.tableComment) {
    lines.push("## Description");
    lines.push("");
    lines.push(parts.tableComment);
    lines.push("");
  }

  // Table Definition
  if (parts.createSql) {
    lines.push("<details>");
    lines.push("<summary><strong>Table Definition</strong></summary>");
    lines.push("");
    lines.push("```sql");
    parts.createSql.split(/\r?\n/).forEach((l) => lines.push(l));
    lines.push("```");
    lines.push("");
    lines.push("</details>");
    lines.push("");
  }

  // Columns
  if (parts.columns.length > 0) {
    lines.push("## Columns");
    lines.push("");
    lines.push("| Name | Type | Default | Nullable | Extra Definition | Children | Parents | Comment |");
    lines.push("| ---- | ---- | ------- | -------- | ---------------- | -------- | ------- | ------- |");

    for (const col of parts.columns) {
      // Backslashes first: otherwise a comment ending in "\" would un-escape the pipe.
      const comment = col.comment.replace(/\\/g, "\\\\").replace(/\|/g, "\\|");

      const children = (parts.fkChildren[col.name] || []).map(c => "[" + c.table + "](" + c.table + ".md)").join(" ");
      const parents = (parts.fkParents[col.name] || []).map(p => "[" + p.table + "](" + p.table + ".md)").join(" ");

      lines.push("| " + col.name + " | " + col.type + " | " + col.defaultStr + " | " + col.nullable + " | " + col.extra + " | " + children + " | " + parents + " | " + comment + " |");
    }
    lines.push("");
  }

  // Constraints
  const constraints = [];
  if (parts.primaryKey) {
    constraints.push({ name: "PRIMARY", type: "PRIMARY KEY", definition: "PRIMARY KEY (" + parts.primaryKey.join(", ") + ")" });
  }
  for (const idx of parts.indexes) {
    const cols = idx.columns.join(", ");
    if (idx.primary) {
      constraints.push({ name: idx.name, type: "PRIMARY KEY", definition: "PRIMARY KEY (" + cols + ")" });
    } else if (idx.unique) {
      constraints.push({ name: idx.name, type: "UNIQUE", definition: "UNIQUE KEY " + idx.name + " (" + cols + ")" });
    }
  }
  for (const [name, fk] of Object.entries(parts.fkConstraints)) {
    constraints.push({
      name: name,
      type: "FOREIGN KEY",
      definition: "FOREIGN KEY (" + fk.columns.join(", ") + ") REFERENCES " + fk.refTable + " (" + fk.refColumns.join(", ") + ")",
    });
  }

  if (constraints.length > 0) {
    lines.push("## Constraints");
    lines.push("");
    lines.push("| Name | Type | Definition |");
    lines.push("| ---- | ---- | ---------- |");
    for (const c of constraints) {
      lines.push("| " + c.name + " | " + c.type + " | " + c.definition + " |");
    }
    lines.push("");
  }

  // Indexes
  if (parts.indexes.length > 0) {
    lines.push("## Indexes");
    lines.push("");
    lines.push("| Name | Definition |");
    lines.push("| ---- | ---------- |");
    for (const idx of parts.indexes) {
      const cols = idx.columns.join(", ");
      let prefix;
      if (idx.unique && idx.primary) prefix = "PRIMARY KEY";
      else if (idx.unique) prefix = "UNIQUE KEY";
      else prefix = "KEY";
      const using = idx.using ? " USING " + idx.using : "";
      lines.push("| " + idx.name + " | " + prefix + " (" + cols + ")" + using + " |");
    }
    lines.push("");
  }

  // Footer
  lines.push("---");
  lines.push("");
  lines.push("> Generated by [muSQL](https://github.com/kan/musql) ([tbls](https://github.com/k1LoW/tbls) compatible)");

  return lines.join(newline || "\n");
}

function showExportMenu(e, columns, rows, tableName) {
  const items = currentRowsExportItems(columns, rows, tableName);
  if (tableName) {
    items.push({ separator: true });
    items.push({ label: t('csv_all'), icon: "download", action: () => doExportAll(tableName, ",", "csv") });
    items.push({ label: t('tsv_all'), icon: "download", action: () => doExportAll(tableName, "\t", "tsv") });
    items.push({ label: t('sql_all'), icon: "download", action: () => doExportSql(tableName) });
    items.push({ separator: true });
    items.push({ label: t('markdown_schema'), icon: "download", action: () => doExportMarkdownSchema(tableName) });
  }
  showContextMenu(e, items);
}

function doExportAll(tableName, sep, ext) {
  openExportDialog({
    defaultName: tableName + "_all." + ext,
    ext,
    filterName: ext.toUpperCase(),
    generate: async (nl) => {
      const res = await runQuery("SELECT * FROM " + quoteId(tableName), 0);
      return generateCsv(res.columns, res.rows, sep, nl);
    },
  });
}

function doExportSql(tableName) {
  openExportDialog({
    defaultName: tableName + ".sql",
    ext: "sql",
    filterName: "SQL",
    generate: async (nl) => generateInsertSql(tableName, nl),
  });
}

function doExportMarkdownSchema(tableName) {
  openExportDialog({
    defaultName: tableName + ".md",
    ext: "md",
    filterName: "Markdown",
    generate: async (nl) => generateMarkdownSchema(tableName, nl),
  });
}

// The result tables (`renderTable`, `renderPagedTable`, `showRowDetailModal`) live in
// result-table.js, shared with the Redis window (#98).

// ── TabManager ──

const tabManager = {
  tabs: [], // { id, type, title, el, paneEl }
  activeId: null,

  addTab(id, type, title, contentBuilder) {
    const existing = this.tabs.find((t) => t.id === id);
    if (existing) {
      this.activate(id);
      return existing;
    }

    // Tab button
    const el = document.createElement("div");
    el.className = "tab-item";
    el.dataset.tabId = id;

    // Tab icon by type
    const tabIcons = { data: 'table', schema: 'columns-3', sql: 'terminal' };
    if (tabIcons[type]) {
      const tabIcon = document.createElement("span");
      tabIcon.innerHTML = icon(tabIcons[type], 14);
      tabIcon.style.display = "flex";
      el.appendChild(tabIcon);
    }

    const labelSpan = document.createElement("span");
    labelSpan.textContent = title;
    el.appendChild(labelSpan);

    const closeBtn = document.createElement("span");
    closeBtn.className = "tab-close";
    closeBtn.innerHTML = icon('x', 14);
    closeBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      this.closeTab(id);
    });
    el.appendChild(closeBtn);

    // ── Tab reorder via mouse drag ──
    let tabDragged = false;
    el.addEventListener("click", () => {
      if (tabDragged) return;
      this.activate(id);
    });
    el.addEventListener("mousedown", (e) => {
      if (e.target.closest(".tab-close")) return;
      if (e.button !== 0) return;

      const startX = e.clientX;
      tabDragged = false;

      const onMove = (me) => {
        if (!tabDragged) {
          if (Math.abs(me.clientX - startX) < 5) return;
          tabDragged = true;
          el.classList.add("dragging");
        }

        tabBarTabs.querySelectorAll(".tab-item").forEach((t) => {
          t.classList.remove("tab-drop-before", "tab-drop-after");
        });
        const target = Array.from(tabBarTabs.querySelectorAll(".tab-item")).find((t) => {
          if (t === el) return false;
          const rect = t.getBoundingClientRect();
          return me.clientX >= rect.left && me.clientX <= rect.right;
        });
        if (target) {
          const rect = target.getBoundingClientRect();
          const mid = rect.left + rect.width / 2;
          if (me.clientX < mid) {
            target.classList.add("tab-drop-before");
          } else {
            target.classList.add("tab-drop-after");
          }
        }
      };

      const onUp = (ue) => {
        document.removeEventListener("mousemove", onMove);
        document.removeEventListener("mouseup", onUp);

        if (!tabDragged) return;
        el.classList.remove("dragging");
        tabBarTabs.querySelectorAll(".tab-item").forEach((t) => {
          t.classList.remove("tab-drop-before", "tab-drop-after");
        });

        const target = Array.from(tabBarTabs.querySelectorAll(".tab-item")).find((t) => {
          if (t === el) return false;
          const rect = t.getBoundingClientRect();
          return ue.clientX >= rect.left && ue.clientX <= rect.right;
        });
        if (target) {
          const targetId = target.dataset.tabId;
          const rect = target.getBoundingClientRect();
          const mid = rect.left + rect.width / 2;
          this._reorderTab(id, targetId, ue.clientX < mid);
        }
      };

      document.addEventListener("mousemove", onMove);
      document.addEventListener("mouseup", onUp);
    });

    // ── Right-click context menu (#42) ──
    el.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      const idx = this.tabs.findIndex((t) => t.id === id);
      const items = [
        { label: t('tab_close'), icon: 'x', action: () => this.closeTab(id) },
      ];
      if (this.tabs.length > 1) {
        items.push({ label: t('tab_close_others'), action: () => this.closeOthers(id) });
      }
      if (idx >= 0 && idx < this.tabs.length - 1) {
        items.push({ label: t('tab_close_right'), action: () => this.closeToRight(id) });
      }
      items.push({ separator: true });
      items.push({ label: t('tab_close_all'), action: () => this.closeAllTabs() });
      showContextMenu(e, items);
    });

    tabBarTabs.appendChild(el);

    // Pane
    const paneEl = document.createElement("div");
    paneEl.className = "tab-pane";
    paneEl.dataset.tabId = id;
    tabContent.appendChild(paneEl);

    const tab = { id, type, title, el, paneEl };
    this.tabs.push(tab);

    if (contentBuilder) contentBuilder(paneEl);

    this.activate(id);
    return tab;
  },

  _reorderTab(draggedId, targetId, insertBefore) {
    const dragIdx = this.tabs.findIndex((t) => t.id === draggedId);
    const targetIdx = this.tabs.findIndex((t) => t.id === targetId);
    if (dragIdx === -1 || targetIdx === -1) return;

    const dragTab = this.tabs[dragIdx];
    const targetTab = this.tabs[targetIdx];

    // Reorder DOM
    if (insertBefore) {
      tabBarTabs.insertBefore(dragTab.el, targetTab.el);
    } else {
      tabBarTabs.insertBefore(dragTab.el, targetTab.el.nextSibling);
    }

    // Reorder array
    this.tabs.splice(dragIdx, 1);
    let newIdx = this.tabs.indexOf(targetTab);
    if (!insertBefore) newIdx++;
    this.tabs.splice(newIdx, 0, dragTab);

    scheduleDraftSave();
  },

  activate(id) {
    this.activeId = id;
    this.tabs.forEach((t) => {
      t.el.classList.toggle("active", t.id === id);
      t.paneEl.classList.toggle("active", t.id === id);
    });
    // Refresh CodeMirror editors in the newly active pane
    const activeTab = this.tabs.find((t) => t.id === id);
    if (activeTab) {
      requestAnimationFrame(() => {
        activeTab.paneEl.querySelectorAll(".CodeMirror").forEach((el) => {
          if (el.CodeMirror) el.CodeMirror.refresh();
        });
      });
    }
    scheduleDraftSave();
  },

  closeTab(id) {
    const idx = this.tabs.findIndex((t) => t.id === id);
    if (idx === -1) return;

    const tab = this.tabs[idx];

    // Save closed SQL tab content for in-session restore
    if (tab.type === "sql") {
      const match = tab.id.match(/^sql-(\d+)$/);
      if (match) {
        const cmEl = tab.paneEl.querySelector(".CodeMirror");
        if (cmEl && cmEl.CodeMirror) {
          const content = cmEl.CodeMirror.getValue();
          if (content) closedDrafts[parseInt(match[1], 10)] = content;
        }
      }
    }

    tab.el.remove();
    tab.paneEl.remove();
    this.tabs.splice(idx, 1);
    delete sqlTabActions[id];

    if (this.activeId === id && this.tabs.length > 0) {
      const nextIdx = Math.min(idx, this.tabs.length - 1);
      this.activate(this.tabs[nextIdx].id);
    } else if (this.tabs.length === 0) {
      this.activeId = null;
    }

    saveDrafts();
  },

  removeAll() {
    // Every path that drops all tabs starts a new notification session (closing the
    // window, switching database): the ids are about to be reused.
    notifySession++;
    this.tabs.forEach((t) => {
      t.el.remove();
      t.paneEl.remove();
      delete sqlTabActions[t.id];
    });
    this.tabs = [];
    this.activeId = null;
    // Clear in-session closed drafts on full reset
    Object.keys(closedDrafts).forEach((k) => delete closedDrafts[k]);
  },

  has(id) {
    return this.tabs.some((t) => t.id === id);
  },

  // Close a set of tabs by id. Collect ids first so index shifts during
  // closeTab don't skip any (#42).
  _closeMany(ids) {
    ids.forEach((id) => this.closeTab(id));
  },
  closeOthers(id) {
    this._closeMany(this.tabs.filter((t) => t.id !== id).map((t) => t.id));
  },
  closeToRight(id) {
    const idx = this.tabs.findIndex((t) => t.id === id);
    if (idx === -1) return;
    this._closeMany(this.tabs.slice(idx + 1).map((t) => t.id));
  },
  closeAllTabs() {
    this._closeMany(this.tabs.map((t) => t.id));
  },
};

// ── DB selection modal ──

async function showDbModal() {
  explorerEl.classList.add("hidden");
  dbModal.classList.remove("hidden");
  dbList.innerHTML = "";
  dbModalStatus.textContent = t('loading');

  try {
    const res = await runQuery("SHOW DATABASES");
    dbModalStatus.textContent = "";
    const dbNames = res.rows.map((r) => r[0]);

    dbNames.forEach((name) => {
      const el = document.createElement("div");
      el.className = "db-list-item";
      el.innerHTML = icon('database', 14);
      el.appendChild(document.createTextNode(' ' + name));
      el.addEventListener("click", () => selectDatabase(name));
      dbList.appendChild(el);
    });
  } catch (error) {
    dbModalStatus.textContent = String(error);
  }
}

async function selectDatabase(dbName) {
  setDatabase(dbName);
  dbModal.classList.add("hidden");
  tabManager.removeAll();
  safeInvoke("clear_schema_cache").catch(() => {});
  await showExplorer();
}

// ── Explorer ──

// A SQLite file is the one database (#97): there is none to choose, so the database list is
// skipped. The file's name stands in for the database name (the title, the sidebar, the AI
// chat's key).
async function openSqliteFile() {
  const path = requestCache.mysql.file_path || "";
  currentDb = path.split(/[\\/]/).pop();
  updateWindowTitle();
  await showExplorer();
}

async function showExplorer() {
  explorerEl.classList.remove("hidden");
  sidebarDbName.textContent = currentDb;
  dbSwitchBtn.classList.toggle("hidden", isSqlite());
  await loadTableList();
  const { tabs, activeTabId } = loadDrafts();
  if (tabs.length > 0) {
    tabs.forEach((d) => {
      if (d.type === "table") {
        openTableTab(d.name, d.view || "data");
      } else {
        addSqlTab(d.content, d.num);
      }
    });
    if (activeTabId && tabManager.has(activeTabId)) {
      tabManager.activate(activeTabId);
    }
  } else {
    addSqlTab();
  }
}

async function loadTableList() {
  currentTables = []; // avoid QuickOpen showing the previous DB's tables during the round-trip
  tableListEl.innerHTML = "";
  const loadingEl = document.createElement("div");
  loadingEl.className = "result-info";
  loadingEl.textContent = t('loading');
  tableListEl.appendChild(loadingEl);
  try {
    const res = await runQuery(tableListSql());
    tableListEl.innerHTML = "";
    const tables = res.rows.map((r) => r[0]);
    currentTables = tables;

    tables.forEach((name) => {
      const el = document.createElement("div");
      el.className = "table-list-item";
      el.innerHTML = icon('table', 14);
      el.appendChild(document.createTextNode(' ' + name));

      el.addEventListener("click", () => openTableTab(name, "data"));

      el.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        e.stopPropagation();
        showContextMenu(e, [
          { label: t('data'), icon: "table", action: () => openTableTab(name, "data") },
          { label: t('schema'), icon: "columns-3", action: () => openTableTab(name, "structure") },
        ]);
      });

      tableListEl.appendChild(el);
    });
  } catch (error) {
    const errEl = document.createElement("div");
    errEl.className = "result-info";
    errEl.textContent = String(error);
    tableListEl.appendChild(errEl);
  }
}

// ── Table tab (Data + Structure unified) ──

function openTableTab(tableName, initialView) {
  const tabId = "table-" + tableName;

  // If already open, just activate
  if (tabManager.has(tabId)) {
    tabManager.activate(tabId);
    return;
  }

  tabManager.addTab(tabId, "data", tableName, (pane) => {
    const body = document.createElement("div");
    body.className = "data-tab-body";

    // View toggle bar (Data | Structure) — right-aligned
    const toggleBar = document.createElement("div");
    toggleBar.className = "view-toggle-bar";
    const toggleLabel = document.createElement("span");
    toggleLabel.style.cssText = "font-size:13px;font-weight:600;";
    toggleLabel.textContent = tableName;
    toggleBar.appendChild(toggleLabel);
    const reloadBtn = document.createElement("button");
    reloadBtn.className = "ghost reload-btn";
    reloadBtn.innerHTML = icon('rotate-cw', 14);
    reloadBtn.title = t('reload');
    toggleBar.appendChild(reloadBtn);

    const toggleGroup = document.createElement("div");
    toggleGroup.className = "view-toggle-group";
    const dataToggle = document.createElement("button");
    dataToggle.className = "view-toggle-btn" + (initialView !== "structure" ? " active" : "");
    dataToggle.textContent = t('data');
    const structToggle = document.createElement("button");
    structToggle.className = "view-toggle-btn" + (initialView === "structure" ? " active" : "");
    structToggle.textContent = t('schema');
    toggleGroup.appendChild(dataToggle);
    toggleGroup.appendChild(structToggle);
    toggleBar.appendChild(toggleGroup);
    body.appendChild(toggleBar);

    const info = document.createElement("div");
    info.className = "result-info";
    info.textContent = t('loading');
    body.appendChild(info);

    const tableContainer = document.createElement("div");
    tableContainer.className = "table-wrap";
    body.appendChild(tableContainer);

    const footerBar = document.createElement("div");
    footerBar.className = "paging-bar";
    body.appendChild(footerBar);

    pane.appendChild(body);

    let currentView = initialView || "data";
    let currentPage = 0;
    let pageSize = 100;
    let totalRows = 0;
    let lastColumns = [];
    let lastRows = [];
    const sortState = { colIndex: -1, dir: null, column: null };

    // Column metadata (fetchColumnMeta)
    let allColumns = [];
    let blobCols = [];
    let textCols = [];
    let pkCols = [];
    let truncateMode = true;
    let hasTruncatable = false;
    let columnsDetected = false;

    async function detectColumns() {
      if (columnsDetected) return;
      allColumns = await fetchColumnMeta(tableName);
      blobCols = allColumns.filter((c) => columnKind(c.dataType) === "blob").map((c) => c.name);
      textCols = allColumns.filter((c) => columnKind(c.dataType) === "text").map((c) => c.name);
      pkCols = allColumns.filter((c) => c.columnKey === "PRI").map((c) => c.name);
      hasTruncatable = blobCols.length > 0 || textCols.length > 0;
      columnsDetected = true;
    }

    function buildSelectExpr() {
      if (!truncateMode || !hasTruncatable) return "*";
      return allColumns.map((c) => {
        const q = quoteId(c.name);
        if (blobCols.includes(c.name)) return "'(BLOB)' AS " + q;
        if (textCols.includes(c.name)) return truncatedTextExpr(q);
        return q;
      }).join(", ");
    }

    function buildOrderByClause() {
      if (sortState.colIndex < 0 || !sortState.dir || !sortState.column) return "";
      return " ORDER BY " + quoteId(sortState.column) + (sortState.dir === "desc" ? " DESC" : " ASC");
    }

    function onRowClick(columns, row) {
      if (pkCols.length > 0) {
        const whereParts = pkCols.map((pk) => {
          const idx = columns.indexOf(pk);
          if (idx === -1) return null;
          const val = row[idx];
          if (val === null || val === undefined) return quoteId(pk) + " IS NULL";
          if (typeof val === "number") return quoteId(pk) + " = " + val;
          return quoteId(pk) + " = " + sqlStringLiteral(val);
        }).filter(Boolean);
        if (whereParts.length > 0) {
          const detailCols = allColumns.map((c) => {
            if (blobCols.includes(c.name)) return "'(BLOB)' AS " + quoteId(c.name);
            return quoteId(c.name);
          }).join(", ");
          runQuery("SELECT " + detailCols + " FROM " + quoteId(tableName) + " WHERE " + whereParts.join(" AND ") + " LIMIT 1")
            .then((res) => {
              showRowDetailModal(res.rows.length > 0 ? res.columns : columns, res.rows.length > 0 ? res.rows[0] : row);
            })
            .catch(() => showRowDetailModal(columns, row));
          return;
        }
      }
      showRowDetailModal(columns, row);
    }

    // Server-side sort handler: overrides renderTable's client-side sort
    function onSort(colIndex, columns) {
      const colName = columns[colIndex];
      if (sortState.colIndex === colIndex) {
        if (sortState.dir === "asc") sortState.dir = "desc";
        else if (sortState.dir === "desc") { sortState.dir = null; sortState.colIndex = -1; sortState.column = null; }
        else sortState.dir = "asc";
      } else {
        sortState.colIndex = colIndex;
        sortState.dir = "asc";
        sortState.column = colName;
      }
      currentPage = 0;
      loadDataView();
    }

    async function loadDataView() {
      info.textContent = t('loading');
      tableContainer.innerHTML = "";
      try {
        await detectColumns();
        if (totalRows === 0) {
          const countRes = await runQuery("SELECT COUNT(*) FROM " + quoteId(tableName));
          totalRows = countRes.rows[0][0];
        }
        const offset = currentPage * pageSize;
        const selectExpr = buildSelectExpr();
        const orderBy = buildOrderByClause();
        const res = await runQuery(
          // LIMIT … OFFSET reads the same in MySQL and SQLite.
          "SELECT " + selectExpr + " FROM " + quoteId(tableName) + orderBy + " LIMIT " + pageSize + " OFFSET " + offset
        );
        lastColumns = res.columns;
        lastRows = res.rows;
        if (totalRows === 0 && res.rows.length === 0) {
          info.textContent = t('zero_rows');
        } else {
          const from = offset + 1;
          const to = offset + res.rows.length;
          info.textContent = t('rows_range', { from: from, to: to, total: totalRows });
        }
        // Use renderTable but disable client-side sort — we handle sort via onSort
        renderTable(res.columns, res.rows, tableContainer, onRowClick, sortState, (ci) => onSort(ci, res.columns));
        renderDataFooter();
      } catch (error) {
        info.textContent = String(error);
      }
    }

    async function loadStructureView() {
      info.textContent = t('loading');
      tableContainer.innerHTML = "";
      footerBar.innerHTML = "";
      try {
        const [descRes, indexRes] = await Promise.all(structureSql(tableName).map((sql) => runQuery(sql)));
        info.textContent = t('n_columns', { n: descRes.rows.length });
        renderTable(descRes.columns, descRes.rows, tableContainer, (cols, row) => showRowDetailModal(cols, row));

        // Index information
        if (indexRes.rows.length > 0) {
          const heading = document.createElement("div");
          heading.className = "index-section-heading";
          heading.textContent = t('indexes');
          tableContainer.appendChild(heading);

          const indexContainer = document.createElement("div");
          indexContainer.className = "table-scroll";
          // Remove "Table" column and move "Key_name" to the front
          const tableIdx = indexRes.columns.indexOf("Table");
          const idxCols = indexRes.columns.slice();
          let idxRows = indexRes.rows.map((r) => r.slice());
          if (tableIdx >= 0) {
            idxCols.splice(tableIdx, 1);
            idxRows = idxRows.map((r) => { r.splice(tableIdx, 1); return r; });
          }
          const knPos = idxCols.indexOf("Key_name");
          if (knPos > 0) {
            idxCols.splice(knPos, 1);
            idxCols.unshift("Key_name");
            idxRows = idxRows.map((r) => { const v = r.splice(knPos, 1)[0]; r.unshift(v); return r; });
          }
          renderTable(idxCols, idxRows, indexContainer);
          tableContainer.appendChild(indexContainer);
        }

        // Markdown export button in footer
        const actions = document.createElement("div");
        actions.className = "footer-actions";
        const mdExportBtn = document.createElement("button");
        mdExportBtn.className = "ghost paging-btn";
        mdExportBtn.innerHTML = icon('download') + t("markdown_schema");
        mdExportBtn.addEventListener("click", () => doExportMarkdownSchema(tableName));
        actions.appendChild(mdExportBtn);
        footerBar.appendChild(actions);
      } catch (error) {
        info.textContent = String(error);
      }
    }

    function renderDataFooter() {
      footerBar.innerHTML = "";
      const totalPages = Math.max(1, Math.ceil(totalRows / pageSize));

      const nav = document.createElement("div");
      nav.className = "paging-nav";
      const prevBtn = document.createElement("button");
      prevBtn.className = "ghost paging-btn";
      prevBtn.innerHTML = icon('chevron-left') + t('prev');
      prevBtn.disabled = currentPage === 0;
      prevBtn.addEventListener("click", () => { currentPage--; loadDataView(); });
      nav.appendChild(prevBtn);
      const pageInfo = document.createElement("span");
      pageInfo.className = "paging-info";
      pageInfo.textContent = (currentPage + 1) + " / " + totalPages;
      nav.appendChild(pageInfo);
      const nextBtn = document.createElement("button");
      nextBtn.className = "ghost paging-btn";
      nextBtn.innerHTML = t('next') + icon('chevron-right');
      nextBtn.disabled = currentPage >= totalPages - 1;
      nextBtn.addEventListener("click", () => { currentPage++; loadDataView(); });
      nav.appendChild(nextBtn);
      footerBar.appendChild(nav);

      const sizeSelector = document.createElement("div");
      sizeSelector.className = "paging-sizes";
      [50, 100, 200, 500].forEach((size) => {
        const btn = document.createElement("button");
        btn.className = "ghost paging-size-btn" + (size === pageSize ? " active" : "");
        btn.textContent = size;
        btn.addEventListener("click", () => {
          if (size === pageSize) return;
          pageSize = size;
          currentPage = 0;
          loadDataView();
        });
        sizeSelector.appendChild(btn);
      });
      footerBar.appendChild(sizeSelector);

      const actions = document.createElement("div");
      actions.className = "footer-actions";
      if (hasTruncatable) {
        const truncBtn = document.createElement("button");
        truncBtn.className = "ghost truncate-btn" + (truncateMode ? " active" : "");
        truncBtn.innerHTML = icon('scissors') + t('truncate');
        truncBtn.title = t('truncate_title');
        truncBtn.addEventListener("click", () => {
          truncateMode = !truncateMode;
          truncBtn.classList.toggle("active", truncateMode);
          loadDataView();
        });
        actions.appendChild(truncBtn);
      }
      const exportBtn = document.createElement("button");
      exportBtn.className = "ghost paging-btn";
      exportBtn.innerHTML = icon('download') + t('export') + ' \u25BE';
      exportBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        showExportMenu(ev, lastColumns, lastRows, tableName);
      });
      actions.appendChild(exportBtn);
      footerBar.appendChild(actions);
    }

    function switchView(view) {
      currentView = view;
      pane.dataset.view = view;
      dataToggle.classList.toggle("active", view === "data");
      structToggle.classList.toggle("active", view === "structure");
      // Update tab icon
      const tab = tabManager.tabs.find((t) => t.id === tabId);
      if (tab) {
        const tabIcon = tab.el.querySelector("span");
        if (tabIcon) tabIcon.innerHTML = icon(view === "structure" ? "columns-3" : "table", 14);
      }
      if (view === "data") {
        loadDataView();
      } else {
        loadStructureView();
      }
      scheduleDraftSave();
    }

    dataToggle.addEventListener("click", () => switchView("data"));
    structToggle.addEventListener("click", () => switchView("structure"));
    reloadBtn.addEventListener("click", () => {
      currentPage = 0;
      totalRows = 0;
      sortState.colIndex = -1;
      sortState.dir = null;
      sortState.column = null;
      columnsDetected = false;
      if (currentView === "data") loadDataView(); else loadStructureView();
    });

    // Initial load
    switchView(currentView);
  });
}

// ── SQL tab ──

function addSqlTab(initialContent, tabNum) {
  let num;
  if (tabNum) {
    num = tabNum;
  } else {
    // Find the lowest unused SQL tab number
    const usedNumbers = new Set();
    tabManager.tabs.forEach((tab) => {
      if (tab.type === "sql") {
        const match = tab.id.match(/^sql-(\d+)$/);
        if (match) usedNumbers.add(parseInt(match[1], 10));
      }
    });
    num = 1;
    while (usedNumbers.has(num)) num++;
  }

  // Determine content: explicit arg > closedDraft > empty
  let content = initialContent;
  if (content === undefined && closedDrafts[num] !== undefined) {
    content = closedDrafts[num];
    delete closedDrafts[num];
  }

  const tabId = "sql-" + num;
  const title = "SQL" + (num > 1 ? " " + num : "");

  tabManager.addTab(tabId, "sql", title, (pane) => {
    const inputArea = document.createElement("div");
    inputArea.className = "sql-input-area sql-editor";

    const editorDiv = document.createElement("div");
    inputArea.appendChild(editorDiv);

    let executing = false;
    let editorTouched = false;

    const editor = CodeMirror(editorDiv, {
      mode: "text/x-" + sqlDialect(),
      lineNumbers: true,
      indentWithTabs: true,
      smartIndent: true,
      lineWrapping: false,
      styleActiveLine: true,
      hintOptions: {
        completeSingle: false,
        tables: buildTableHints(),
      },
      extraKeys: {
        "Tab": function (cm) {
          cm.replaceSelection("\t");
        },
        "Ctrl-Enter": function () {
          if (executing) return;
          const stmt = getStatementAtCursor();
          if (stmt) executeStatements([stmt]);
        },
        "Ctrl-Space": "autocomplete",
      },
    });

    editor.on("focus", function () { editorTouched = true; });
    editor.on("mousedown", function () { editorTouched = true; });

    // Highlight the current SQL statement (delimited by ;)
    let stmtHighlightLines = [];
    function highlightCurrentStatement() {
      // Clear previous highlights
      stmtHighlightLines.forEach(function (lh) {
        editor.removeLineClass(lh, "background", "cm-statement-highlight");
      });
      stmtHighlightLines = [];

      const text = editor.getValue();
      if (!text.trim()) return;
      const cursor = editor.indexFromPos(editor.getCursor());
      let start = 0;
      const parts = text.split(";");
      for (let i = 0; i < parts.length; i++) {
        const end = start + parts[i].length;
        if (cursor <= end) {
          // Found the statement containing the cursor
          const fromLine = editor.posFromIndex(start).line;
          const toLine = editor.posFromIndex(end).line;
          for (let ln = fromLine; ln <= toLine; ln++) {
            stmtHighlightLines.push(editor.addLineClass(ln, "background", "cm-statement-highlight"));
          }
          break;
        }
        start = end + 1; // skip the ;
      }
    }
    editor.on("cursorActivity", highlightCurrentStatement);

    editor.on("change", function () {
      scheduleDraftSave();
    });

    editor.on("inputRead", function (cm, change) {
      if (change.origin !== "+input") return;
      const ch = change.text[change.text.length - 1];
      if (/[a-zA-Z_.]/.test(ch)) {
        cm.setOption("hintOptions", {
          completeSingle: false,
          tables: buildTableHints(),
        });
        cm.showHint({ completeSingle: false });
      }
    });

    if (content) editor.setValue(content);

    // ── Resize handle ──
    const resizeHandle = document.createElement("div");
    resizeHandle.className = "sql-resize-handle";
    inputArea.appendChild(resizeHandle);

    resizeHandle.addEventListener("mousedown", (startE) => {
      startE.preventDefault();
      const cmWrap = editorDiv.querySelector(".CodeMirror");
      if (!cmWrap) return;
      const startY = startE.clientY;
      const startH = cmWrap.offsetHeight;
      function onMove(e) {
        const newH = Math.max(80, startH + (e.clientY - startY));
        cmWrap.style.height = newH + "px";
        editor.refresh();
      }
      function onUp() {
        document.removeEventListener("mousemove", onMove);
        document.removeEventListener("mouseup", onUp);
      }
      document.addEventListener("mousemove", onMove);
      document.addEventListener("mouseup", onUp);
    });

    const actions = document.createElement("div");
    actions.className = "actions actions-right";

    const historyBtn = document.createElement("button");
    historyBtn.className = "ghost sql-history-btn";
    historyBtn.innerHTML = icon('clock') + t('history') + ' \u25BE';
    historyBtn.addEventListener("click", (ev) => {
      ev.stopPropagation();
      const history = loadHistory();
      if (history.length === 0) {
        showContextMenu(ev, [{ label: t('no_history'), action: () => {} }]);
        return;
      }
      showHistoryMenu(ev, history, (sql) => addSqlTab(sql));
    });
    actions.appendChild(historyBtn);

    // Spacer to push remaining buttons to the right
    const spacer = document.createElement("div");
    spacer.style.flex = "1";
    actions.appendChild(spacer);

    const formatBtn = document.createElement("button");
    formatBtn.className = "ghost";
    formatBtn.innerHTML = icon('wand-2') + t('format');
    formatBtn.addEventListener("click", () => {
      const selection = editor.getSelection();
      if (selection) {
        const formatted = window.sqlFormatter.format(selection, { language: sqlDialect() });
        editor.replaceSelection(formatted);
      } else {
        const cursor = editor.getCursor();
        const formatted = window.sqlFormatter.format(editor.getValue(), { language: sqlDialect() });
        editor.setValue(formatted);
        editor.setCursor(cursor);
      }
    });
    actions.appendChild(formatBtn);

    const aiAssistBtn = document.createElement("button");
    aiAssistBtn.className = "ghost";
    aiAssistBtn.innerHTML = icon('sparkles') + " " + t('ai_assist');
    aiAssistBtn.addEventListener("click", () => {
      const database = currentDb;
      openAiAssistModal({
        chatKey: aiChatKey(currentProfileId, database),
        placeholder: t('ai_assist_placeholder'),
        emptyText: t('ai_assist_empty'),
        request: (prompt, conversationContext, ai) => safeInvoke("ai_assist", {
          prompt,
          editorContent: editor.getValue(),
          conversationContext,
          database,
          ...ai,
        }),
        insertLabel: t('ai_assist_insert'),
        insert: (text) => {
          editor.replaceRange(text + "\n", editor.getCursor());
          editor.focus();
        },
      });
    });
    actions.appendChild(aiAssistBtn);

    let cancelled = false;

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "danger";
    cancelBtn.innerHTML = icon('x-circle') + t('cancel');
    cancelBtn.style.display = "none";
    cancelBtn.addEventListener("click", () => {
      cancelBtn.disabled = true;
      cancelled = true;
      safeInvoke("cancel_query", { tabId }).catch(() => {});
    });
    actions.appendChild(cancelBtn);

    const runLineBtn = document.createElement("button");
    runLineBtn.className = "info";
    runLineBtn.innerHTML = icon('play') + t('run_this_line');
    actions.appendChild(runLineBtn);

    const runAllBtn = document.createElement("button");
    runAllBtn.innerHTML = icon('play') + t('run_all');
    actions.appendChild(runAllBtn);

    const exportBtn = document.createElement("button");
    exportBtn.className = "ghost";
    exportBtn.innerHTML = icon('download') + t('export') + ' \u25BE';
    exportBtn.style.display = "none";
    actions.appendChild(exportBtn);

    inputArea.appendChild(actions);

    const resultArea = document.createElement("div");
    resultArea.className = "sql-result-area";

    pane.appendChild(inputArea);
    pane.appendChild(resultArea);

    // Refresh CodeMirror once the pane is visible
    requestAnimationFrame(() => editor.refresh());

    let lastColumns = [];
    let lastRows = [];
    let errorMarks = [];

    function clearErrorMarks() {
      errorMarks.forEach((m) => m.clear());
      errorMarks = [];
    }

    function markSqlError(errorMsg) {
      const pos = sqlErrorPosition(errorMsg);
      if (!pos) return;
      const nearText = pos.near;
      const errorLine = pos.line;
      if (nearText.length === 0) return;

      // Find nearText in the editor starting from errorLine
      const lineCount = editor.lineCount();
      for (let ln = errorLine; ln < lineCount; ln++) {
        const lineText = editor.getLine(ln);
        const col = lineText.toLowerCase().indexOf(nearText.toLowerCase().split(/\s/)[0]);
        if (col !== -1) {
          // Mark the first word of the near text
          const word = nearText.split(/\s/)[0];
          const from = { line: ln, ch: col };
          const to = { line: ln, ch: col + word.length };
          errorMarks.push(editor.markText(from, to, { className: "cm-sql-error" }));
          return;
        }
      }
    }

    function getStatementAtCursor() {
      const text = editor.getValue();
      const cursor = editor.indexFromPos(editor.getCursor());
      let start = 0;
      const parts = text.split(";");
      for (let i = 0; i < parts.length; i++) {
        const end = start + parts[i].length;
        if (cursor <= end) return parts[i].trim();
        start = end + 1;
      }
      return parts[parts.length - 1].trim();
    }

    function getAllStatements() {
      return editor.getValue().split(";").map((s) => s.trim()).filter((s) => s.length > 0);
    }

    function setExecuting(state) {
      executing = state;
      runLineBtn.disabled = state;
      runAllBtn.disabled = state;
      cancelBtn.style.display = state ? "" : "none";
      if (state) cancelBtn.disabled = false;
    }

    function makeResultInfo(text, elapsed) {
      const info = document.createElement("div");
      info.className = "result-info";
      const span = document.createElement("span");
      span.textContent = text;
      info.appendChild(span);
      if (elapsed != null) {
        const timeSpan = document.createElement("span");
        timeSpan.className = "exec-time";
        timeSpan.textContent = elapsed < 1000 ? Math.round(elapsed) + " ms" : (elapsed / 1000).toFixed(2) + " s";
        info.appendChild(timeSpan);
      }
      return info;
    }

    async function executeStatements(statements) {
      const session = notifySession; // for the completion toast, see maybeNotifyQueryDone
      resultArea.innerHTML = "";
      lastColumns = [];
      lastRows = [];
      exportBtn.style.display = "none";
      clearErrorMarks();
      cancelled = false;
      if (statements.length === 0) return;

      setExecuting(true);
      const multi = statements.length > 1;
      const loadingEl = document.createElement("div");
      loadingEl.className = "result-info";
      loadingEl.textContent = t('running');
      resultArea.appendChild(loadingEl);
      const execStart = performance.now();
      let notifyBody = null;

      try {
        for (const sql of statements) {
          const stmtStart = performance.now();
          // maxRows = 0 → fetch all rows; on-screen paging is client-side (see renderPagedTable),
          // and export uses the full in-memory set (issue #41).
          const res = await runQuery(sql, 0, tabId);
          const stmtElapsed = performance.now() - stmtStart;

          if (cancelled) {
            loadingEl.remove();
            const info = document.createElement("div");
            info.className = "result-info";
            info.style.color = "var(--danger)";
            info.textContent = t('query_cancelled');
            resultArea.appendChild(info);
            return;
          }

          saveHistory(sql);

          if (res.columns && res.columns.length > 0) {
            lastColumns = res.columns;
            lastRows = res.rows;

            if (multi) {
              const header = document.createElement("div");
              header.className = "result-info sql-result-query";
              header.textContent = sql.length > 100 ? sql.substring(0, 100) + "\u2026" : sql;
              resultArea.appendChild(header);
            }

            resultArea.appendChild(makeResultInfo(t('n_rows', { n: res.rows.length }), stmtElapsed));

            const wrapper = document.createElement("div");
            resultArea.appendChild(wrapper);
            renderPagedTable(res.columns, res.rows, wrapper, (cols, row) => showRowDetailModal(cols, row));
          } else {
            const prefix = multi ? (sql.length > 60 ? sql.substring(0, 60) + "\u2026" : sql) + " \u2192 " : "";
            resultArea.appendChild(makeResultInfo(prefix + t('affected_rows', { n: res.affected_rows != null ? res.affected_rows : 0 }), stmtElapsed));
          }
          // Notification body reflects the last statement of the run.
          notifyBody = (res.columns && res.columns.length > 0)
            ? t('notify_query_rows', { n: res.rows.length })
            : t('notify_query_ok');
        }
        loadingEl.remove();
        if (lastColumns.length > 0) {
          exportBtn.style.display = "";
        }
      } catch (error) {
        loadingEl.remove();
        const totalElapsed = performance.now() - execStart;
        const errInfo = makeResultInfo(cancelled ? t('query_cancelled') : String(error), totalElapsed);
        errInfo.style.color = "var(--danger)";
        resultArea.appendChild(errInfo);
        if (!cancelled) markSqlError(String(error));
        notifyBody = t('notify_query_failed');
      } finally {
        setExecuting(false);
        if (!cancelled && notifyBody) {
          maybeNotifyQueryDone(performance.now() - execStart, notifyBody, tabId, session);
        }
      }
    }

    runLineBtn.addEventListener("click", () => {
      if (executing) return;
      if (!editorTouched) {
        const stmts = getAllStatements();
        if (stmts.length === 1) {
          executeStatements([stmts[0]]);
        } else if (stmts.length > 1) {
          resultArea.innerHTML = "";
          const info = document.createElement("div");
          info.className = "result-info";
          info.style.color = "var(--danger)";
          info.textContent = t('place_cursor_hint');
          resultArea.appendChild(info);
          editor.focus();
        }
        return;
      }
      const stmt = getStatementAtCursor();
      if (stmt) executeStatements([stmt]);
    });

    runAllBtn.addEventListener("click", () => {
      if (executing) return;
      const stmts = getAllStatements();
      if (stmts.length > 0) executeStatements(stmts);
    });

    exportBtn.addEventListener("click", (ev) => {
      ev.stopPropagation();
      if (lastColumns.length === 0) return;
      showContextMenu(ev, [
        { label: "CSV", icon: "download", action: () => doExportCurrent(lastColumns, lastRows, null, ",", "csv") },
        { label: "TSV", icon: "download", action: () => doExportCurrent(lastColumns, lastRows, null, "\t", "tsv") },
      ]);
    });

    // Register actions for menu event routing
    sqlTabActions[tabId] = {
      runLine: () => runLineBtn.click(),
      runAll: () => runAllBtn.click(),
      format: () => formatBtn.click(),
      history: () => historyBtn.click(),
      cancel: () => cancelBtn.click(),
    };
  });
}

// ── QuickOpen command palette (Ctrl+P) (#44) ──
// Subsequence fuzzy score: -1 if not all query chars appear in order, else higher-is-better
// with bonuses for prefix and consecutive matches.
function fuzzyScore(query, text) {
  if (!query) return 0;
  const q = query.toLowerCase();
  const s = text.toLowerCase();
  let qi = 0, score = 0, prev = -2;
  for (let si = 0; si < s.length && qi < q.length; si++) {
    if (s[si] === q[qi]) {
      score += si === prev + 1 ? 2 : 1;
      if (si === 0) score += 3;
      prev = si;
      qi++;
    }
  }
  return qi === q.length ? score : -1;
}

function getActiveSqlEditor() {
  const tab = tabManager.tabs.find((tb) => tb.id === tabManager.activeId);
  if (!tab || tab.type !== "sql") return null;
  const cmEl = tab.paneEl.querySelector(".CodeMirror");
  return cmEl && cmEl.CodeMirror ? cmEl.CodeMirror : null;
}

function insertSqlFromHistory(sql) {
  let ed = getActiveSqlEditor();
  if (!ed) {
    // Reuse an existing SQL tab if the active tab isn't one; only create a new tab
    // when there is no SQL tab at all.
    const sqlTab = tabManager.tabs.find((tb) => tb.type === "sql");
    if (sqlTab) {
      tabManager.activate(sqlTab.id);
      const cmEl = sqlTab.paneEl.querySelector(".CodeMirror");
      ed = cmEl && cmEl.CodeMirror ? cmEl.CodeMirror : null;
    }
  }
  if (ed) {
    ed.replaceRange(sql + "\n", ed.getCursor());
    ed.focus();
  } else {
    addSqlTab(sql);
  }
}

let quickOpenEl = null;
let quickOpenClose = null;
function openQuickOpen() {
  if (quickOpenEl) return; // already open
  if (explorerEl.classList.contains("hidden")) return;

  const overlay = document.createElement("div");
  overlay.className = "modal-overlay quickopen-overlay";
  const box = document.createElement("div");
  box.className = "quickopen-box";
  const input = document.createElement("input");
  input.className = "quickopen-input";
  input.type = "text";
  input.placeholder = t('quickopen_placeholder');
  box.appendChild(input);
  const listEl = document.createElement("div");
  listEl.className = "quickopen-list";
  box.appendChild(listEl);
  overlay.appendChild(box);
  document.body.appendChild(overlay);
  quickOpenEl = overlay;

  let items = [];
  let selected = 0;
  let lastPointerX = -1;
  let lastPointerY = -1;

  function close() {
    overlay.remove();
    document.removeEventListener("keydown", onKey, true);
    quickOpenEl = null;
    quickOpenClose = null;
  }
  quickOpenClose = close;

  function byScore(query, arr, textOf) {
    return arr
      .map((v) => ({ v, score: fuzzyScore(query, textOf(v)) }))
      .filter((x) => x.score >= 0)
      .sort((a, b) => b.score - a.score);
  }

  function computeItems() {
    const raw = input.value;
    let mode = "table";
    let query = raw;
    if (raw.startsWith("@")) { mode = "tab"; query = raw.slice(1); }
    else if (raw.startsWith(">")) { mode = "history"; query = raw.slice(1); }
    else if (raw.startsWith("?")) { mode = "help"; query = raw.slice(1); }
    query = query.trim();

    if (mode === "help") {
      items = [
        { label: t('quickopen_help_tables'), sub: "", icon: "table", action: () => { input.value = ""; onInput(); } },
        { label: t('quickopen_help_tabs'), sub: "@", icon: "columns-3", action: () => { input.value = "@"; onInput(); } },
        { label: t('quickopen_help_history'), sub: ">", icon: "clock", action: () => { input.value = ">"; onInput(); } },
      ];
    } else if (mode === "tab") {
      items = byScore(query, tabManager.tabs, (tb) => tb.title).map((x) => ({
        label: x.v.title,
        sub: "",
        icon: x.v.type === "sql" ? "terminal" : "table",
        action: () => tabManager.activate(x.v.id),
      }));
    } else if (mode === "history") {
      items = byScore(query, loadHistory(), (h) => h.sql).slice(0, 50).map((x) => ({
        label: x.v.sql.replace(/\s+/g, " ").slice(0, 120),
        sub: "",
        icon: "clock",
        action: () => insertSqlFromHistory(x.v.sql),
      }));
    } else {
      items = byScore(query, currentTables, (n) => n).slice(0, 100).map((x) => ({
        label: x.v,
        sub: "",
        icon: "table",
        action: () => openTableTab(x.v, "data"),
      }));
    }
    if (selected >= items.length) selected = Math.max(0, items.length - 1);
  }

  function render() {
    listEl.innerHTML = "";
    if (items.length === 0) {
      const empty = document.createElement("div");
      empty.className = "quickopen-empty";
      empty.textContent = t('quickopen_no_results');
      listEl.appendChild(empty);
      return;
    }
    items.forEach((it, i) => {
      const row = document.createElement("div");
      row.className = "quickopen-item" + (i === selected ? " selected" : "");
      const ic = document.createElement("span");
      ic.className = "quickopen-item-icon";
      ic.innerHTML = icon(it.icon, 14);
      row.appendChild(ic);
      const lbl = document.createElement("span");
      lbl.className = "quickopen-item-label";
      lbl.textContent = it.label;
      row.appendChild(lbl);
      if (it.sub) {
        const sub = document.createElement("span");
        sub.className = "quickopen-item-sub";
        sub.textContent = it.sub;
        row.appendChild(sub);
      }
      row.addEventListener("mousemove", (e) => {
        // Ignore synthetic mousemove from scrollIntoView (same pointer coords) so it
        // doesn't fight keyboard navigation.
        if (e.clientX === lastPointerX && e.clientY === lastPointerY) return;
        lastPointerX = e.clientX;
        lastPointerY = e.clientY;
        if (selected !== i) { selected = i; updateSelection(); }
      });
      row.addEventListener("click", () => choose(i));
      listEl.appendChild(row);
    });
  }

  function updateSelection() {
    Array.from(listEl.children).forEach((c, i) => c.classList.toggle("selected", i === selected));
    const sel = listEl.children[selected];
    if (sel && sel.scrollIntoView) sel.scrollIntoView({ block: "nearest" });
  }

  function choose(i) {
    const it = items[i];
    if (!it) return;
    const keepOpen = input.value.startsWith("?"); // help mode just switches the prefix
    it.action();
    if (!keepOpen) close();
  }

  function onInput() {
    selected = 0;
    computeItems();
    render();
  }

  function onKey(e) {
    // stopPropagation so the handled keys don't leak to the bubble-phase Escape
    // handler (which would also close the AI assist modal if it were open).
    if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); close(); }
    else if (e.key === "ArrowDown") { e.preventDefault(); e.stopPropagation(); if (items.length) { selected = (selected + 1) % items.length; updateSelection(); } }
    else if (e.key === "ArrowUp") { e.preventDefault(); e.stopPropagation(); if (items.length) { selected = (selected - 1 + items.length) % items.length; updateSelection(); } }
    else if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); choose(selected); }
  }

  input.addEventListener("input", onInput);
  document.addEventListener("keydown", onKey, true);
  overlay.addEventListener("mousedown", (e) => { if (e.target === overlay) close(); });
  onInput();
  input.focus();
}

document.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && (e.key === "p" || e.key === "P")) {
    e.preventDefault();
    openQuickOpen();
  }
}, true);

// ── Reset ──

function resetExplorer() {
  if (quickOpenClose) quickOpenClose();
  saveDrafts();
  tabManager.removeAll();
  currentDb = null;
  currentProfileId = null;
  currentProfileName = null;
  requestCache = null;
  explorerEl.classList.add("hidden");
  dbModal.classList.add("hidden");
  tableListEl.innerHTML = "";
  sidebarDbName.textContent = "";
  updateWindowTitle();
  safeInvoke("disconnect_pool").catch(() => {});
  if (dockerTunnelContainerId) {
    safeInvoke("docker_stop_tunnel", { containerId: dockerTunnelContainerId }).catch(() => {});
    dockerTunnelContainerId = null;
  }
}

// ── Event wiring ──

dbSwitchBtn.addEventListener("click", () => {
  // The menu's "switch database" clicks this even while it is hidden.
  if (isSqlite()) return;
  saveDrafts();
  tabManager.removeAll();
  showDbModal();
});

tabAddBtn.addEventListener("click", () => addSqlTab());

// ── Tauri event ──

// Note: unlisten is not needed — windows use show/hide pattern, so listeners register once.
if (eventApi && eventApi.listen) {
  eventApi.listen("query:open", async (event) => {
    const id = event.payload || "";
    if (!id) {
      resetExplorer();
      return;
    }

    resetExplorer();
    currentProfileId = id;
    try {
      await loadProfile(id);

      // Prompt for credentials if any save_* flags are false
      const ok = await promptForCredentials(requestCache);
      if (!ok) {
        resetExplorer();
        return;
      }

      // If profile already has a database set, auto-select it
      if (isSqlite()) {
        await openSqliteFile();
      } else if (requestCache.mysql.database) {
        setDatabase(requestCache.mysql.database);
        await showExplorer();
      } else {
        await showDbModal();
      }
    } catch (error) {
      dbModal.classList.remove("hidden");
      dbModalStatus.textContent = String(error);
    }
  });

  eventApi.listen("query:docker-open", async (event) => {
    const info = event.payload;
    if (!info) return;

    resetExplorer();
    dockerTunnelContainerId = info.tunnel_container_id || null;
    currentProfileId = "docker:" + info.name;
    currentProfileName = info.name;

    // Build a synthetic requestCache matching ConnectionRequest shape. The engine is the
    // container's (mysql, or mariadb from a musql.engine label): detection goes by the
    // port and labels, not by asking the server.
    requestCache = {
      engine: info.engine,
      mysql: {
        host: info.host,
        port: info.port,
        database: null,
        username: info.user,
        password: info.password,
        ssl_mode: info.ssl_mode || "REQUIRED",
        tls_ca_cert_path: null,
        save_password: true,
      },
      ssh: null,
    };

    try {
      await showDbModal();
    } catch (error) {
      dbModal.classList.remove("hidden");
      dbModalStatus.textContent = String(error);
    }
  });

  eventApi.listen("query:reset", () => {
    // A prompt left open would answer for the next open (credential-prompt.js).
    cancelCredentialPrompt();
    resetExplorer();
  });

  // A query-done toast was clicked (#113). Rust has already focused the window; the tab
  // may have been closed since the toast was shown.
  eventApi.listen("toast:activated", (event) => {
    const token = String(event.payload);
    const sep = token.indexOf(":");
    if (Number(token.slice(0, sep)) !== notifySession) return; // from an earlier session
    const tabId = token.slice(sep + 1);
    if (tabManager.has(tabId)) tabManager.activate(tabId);
  });

  eventApi.listen("menu:action", (event) => {
    const activeId = tabManager.activeId;
    const actions = sqlTabActions[activeId];
    switch (event.payload) {
      case "new-sql-tab": addSqlTab(); break;
      case "switch-db": dbSwitchBtn.click(); break;
      case "ai-settings": showAiModal(); break;
      case "run": if (actions) actions.runLine(); break;
      case "run-all": if (actions) actions.runAll(); break;
      case "cancel": if (actions) actions.cancel(); break;
      case "format": if (actions) actions.format(); break;
      case "theme-light": setTheme("light"); break;
      case "theme-dark": setTheme("dark"); break;
      case "lang-en": setLang("en"); break;
      case "lang-ja": setLang("ja"); break;
      case "toggle-notify": setNotifyEnabled(!isNotifyEnabled()); break;
    }
  });
}

// Re-render on language change
window.addEventListener("musql:langchange", () => {
  document.getElementById("db-modal-heading").innerHTML = icon('database', 20) + ' ' + t('select_database');
  applyI18n();
});
