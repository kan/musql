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
const contextMenuEl = document.getElementById("context-menu");

const menuBtn = document.getElementById("menu-btn");

// Apply icons to static elements
menuBtn.innerHTML = icon('menu');
dbSwitchBtn.innerHTML = icon('arrow-left-right');
tabAddBtn.innerHTML = icon('plus');

menuBtn.addEventListener("click", () => safeInvoke("show_popup_menu", { lang: getLang(), theme: getTheme() }));
document.getElementById("db-modal-heading").innerHTML = icon('database', 20) + ' ' + t('select_database');

// ── State ──
let requestCache = null;
let currentDb = null;
let sqlTabCounter = 0;
let currentTables = [];
let currentProfileId = null;
let currentProfileName = null;
const sqlTabActions = {}; // tabId → { runLine, runAll, format, history, cancel }

// ── Draft save/restore ──

let draftSaveTimer = null;

function saveDrafts() {
  if (!currentProfileId) return;
  const drafts = [];
  tabManager.tabs.forEach((tab) => {
    if (tab.type !== "sql") return;
    const cmEl = tab.paneEl.querySelector(".CodeMirror");
    if (cmEl && cmEl.CodeMirror) drafts.push(cmEl.CodeMirror.getValue());
  });
  const key = "musql:drafts:" + currentProfileId;
  if (drafts.length === 0 || drafts.every((d) => d === "")) {
    localStorage.removeItem(key);
  } else {
    localStorage.setItem(key, JSON.stringify(drafts));
  }
}

function loadDrafts() {
  if (!currentProfileId) return [];
  try { return JSON.parse(localStorage.getItem("musql:drafts:" + currentProfileId)) || []; }
  catch (_) { return []; }
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

async function loadProfile(id) {
  const data = await safeInvoke("list_profiles");
  const profile = data.items.find((item) => item.id === id);
  if (!profile) throw new Error("profile not found");
  requestCache = profile.request;
  currentProfileName = profile.name;
}

function runQuery(sql, maxRows) {
  const payload = { request: requestCache, query: sql, profileId: currentProfileId || null };
  if (maxRows != null) payload.maxRows = maxRows;
  return safeInvoke("run_query", payload);
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
  const win = window.__TAURI__ && window.__TAURI__.webviewWindow;
  if (win) {
    win.getCurrentWebviewWindow().setTitle(title).catch(() => {});
  }
}

// ── Export utilities ──

function generateCsv(columns, rows, separator) {
  const escape = (val) => {
    if (val === null || val === undefined) return "";
    const s = String(val);
    if (s.includes(separator) || s.includes('"') || s.includes("\n") || s.includes("\r")) {
      return '"' + s.replace(/"/g, '""') + '"';
    }
    return s;
  };
  const lines = [];
  lines.push(columns.map(escape).join(separator));
  rows.forEach((row) => {
    lines.push(columns.map((_, i) => escape(row[i])).join(separator));
  });
  return lines.join("\n");
}

function quoteId(name) {
  return '`' + name.replace(/`/g, '``') + '`';
}

function sqlEscapeValue(val) {
  if (val === null || val === undefined) return "NULL";
  if (typeof val === "number") return String(val);
  return "'" + String(val).replace(/\\/g, "\\\\").replace(/'/g, "\\'") + "'";
}

async function generateInsertSql(tableName) {
  let createSql = "";
  try {
    const res = await runQuery("SHOW CREATE TABLE " + quoteId(tableName));
    if (res.rows.length > 0) createSql = res.rows[0][1];
  } catch (_) { /* ignore */ }

  const dataRes = await runQuery("SELECT * FROM " + quoteId(tableName), 0);
  const columns = dataRes.columns;
  const rows = dataRes.rows;

  const lines = [];
  lines.push("-- mysqldump-compatible export");
  lines.push("-- Table: " + tableName);
  lines.push("-- Exported: " + new Date().toISOString());
  lines.push("");
  lines.push("DROP TABLE IF EXISTS " + quoteId(tableName) + ";");
  lines.push("");
  if (createSql) {
    lines.push(createSql + ";");
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
  return lines.join("\n");
}

async function saveFile(content, defaultName, filterName, extensions) {
  return safeInvoke("export_file", { content, defaultName, filterName, extensions });
}

function showExportMenu(e, columns, rows, tableName) {
  const items = [
    { label: t('csv_current'), icon: "download", action: () => doExportCurrent(columns, rows, tableName, ",", "csv") },
    { label: t('tsv_current'), icon: "download", action: () => doExportCurrent(columns, rows, tableName, "\t", "tsv") },
  ];
  if (tableName) {
    items.push({ separator: true });
    items.push({ label: t('csv_all'), icon: "download", action: () => doExportAll(tableName, ",", "csv") });
    items.push({ label: t('tsv_all'), icon: "download", action: () => doExportAll(tableName, "\t", "tsv") });
    items.push({ label: t('sql_all'), icon: "download", action: () => doExportSql(tableName) });
  }
  showContextMenu(e, items);
}

async function doExportCurrent(columns, rows, tableName, sep, ext) {
  const content = generateCsv(columns, rows, sep);
  const name = (tableName || "export") + "." + ext;
  const filterName = ext.toUpperCase();
  await saveFile(content, name, filterName, [ext]);
}

async function doExportAll(tableName, sep, ext) {
  const res = await runQuery("SELECT * FROM " + quoteId(tableName), 0);
  const content = generateCsv(res.columns, res.rows, sep);
  await saveFile(content, tableName + "_all." + ext, ext.toUpperCase(), [ext]);
}

async function doExportSql(tableName) {
  const content = await generateInsertSql(tableName);
  await saveFile(content, tableName + ".sql", "SQL", ["sql"]);
}

// ── Row detail modal ──

function showRowDetailModal(columns, row) {
  const overlay = document.createElement("div");
  overlay.className = "modal-overlay";

  const box = document.createElement("div");
  box.className = "row-detail-box";

  // Header
  const header = document.createElement("div");
  header.className = "row-detail-header";
  const h3 = document.createElement("h3");
  h3.textContent = t('row_detail');
  header.appendChild(h3);
  const closeBtn = document.createElement("button");
  closeBtn.className = "row-detail-close";
  closeBtn.innerHTML = icon('x');
  header.appendChild(closeBtn);
  box.appendChild(header);

  // Body
  const body = document.createElement("div");
  body.className = "row-detail-body";
  const grid = document.createElement("div");
  grid.className = "row-detail-grid";

  columns.forEach((col, i) => {
    const keyEl = document.createElement("div");
    keyEl.className = "row-detail-key";
    keyEl.textContent = col;
    grid.appendChild(keyEl);

    const valEl = document.createElement("div");
    valEl.className = "row-detail-val";
    const val = row[i];

    if (val === null || val === undefined) {
      valEl.classList.add("null-value");
      valEl.textContent = "NULL";
    } else if (val === "") {
      valEl.classList.add("null-value");
      valEl.textContent = "EMPTY";
    } else {
      const s = String(val);
      // Try to detect and pretty-print JSON
      if ((s.startsWith("{") || s.startsWith("[")) && s.length > 2) {
        try {
          const parsed = JSON.parse(s);
          valEl.textContent = JSON.stringify(parsed, null, 2);
          valEl.classList.add("monospace");
        } catch (_) {
          valEl.textContent = s;
        }
      } else {
        valEl.textContent = s;
      }
      // Long values get monospace for readability
      if (s.length > 200 && !valEl.classList.contains("monospace")) {
        valEl.classList.add("monospace");
      }
    }
    grid.appendChild(valEl);
  });

  body.appendChild(grid);
  box.appendChild(body);
  overlay.appendChild(box);
  document.body.appendChild(overlay);

  function close() {
    overlay.remove();
    document.removeEventListener("keydown", onKey);
  }
  function onKey(e) {
    if (e.key === "Escape") close();
  }
  document.addEventListener("keydown", onKey);
  closeBtn.addEventListener("click", close);
  overlay.addEventListener("click", (e) => {
    if (e.target === overlay) close();
  });
}

// ── renderTable utility ──

function renderTable(columns, rows, container, onRowClick, sortState) {
  container.innerHTML = "";
  const scrollDiv = document.createElement("div");
  scrollDiv.className = "table-scroll";

  const table = document.createElement("table");
  table.className = "result-table";
  if (onRowClick) table.classList.add("clickable-rows");

  // Sort state — use external object if provided, else local
  const ss = sortState || { colIndex: -1, dir: null };

  const thead = document.createElement("thead");
  const headerRow = document.createElement("tr");
  const thEls = [];
  columns.forEach((col, ci) => {
    const th = document.createElement("th");
    th.classList.add("sortable");

    const label = document.createTextNode(col);
    th.appendChild(label);

    const indicator = document.createElement("span");
    indicator.className = "sort-indicator";
    // Restore indicator from existing state
    if (ci === ss.colIndex && ss.dir === "asc") { indicator.textContent = " \u25B2"; th.classList.add("sort-active"); }
    else if (ci === ss.colIndex && ss.dir === "desc") { indicator.textContent = " \u25BC"; th.classList.add("sort-active"); }
    th.appendChild(indicator);

    th.addEventListener("click", () => {
      if (ss.colIndex === ci) {
        if (ss.dir === "asc") ss.dir = "desc";
        else if (ss.dir === "desc") { ss.dir = null; ss.colIndex = -1; }
        else ss.dir = "asc";
      } else {
        ss.colIndex = ci;
        ss.dir = "asc";
      }
      // Update header indicators
      thEls.forEach((t, ti) => {
        const ind = t.querySelector(".sort-indicator");
        t.classList.toggle("sort-active", ti === ss.colIndex && ss.dir !== null);
        if (ti === ss.colIndex && ss.dir === "asc") ind.textContent = " \u25B2";
        else if (ti === ss.colIndex && ss.dir === "desc") ind.textContent = " \u25BC";
        else ind.textContent = "";
      });
      rebuildTbody();
    });

    thEls.push(th);
    headerRow.appendChild(th);
  });
  thead.appendChild(headerRow);
  table.appendChild(thead);

  const tbody = document.createElement("tbody");
  table.appendChild(tbody);

  function getSortedRows() {
    if (ss.colIndex < 0 || ss.dir === null) return rows;
    const sorted = rows.slice();
    const ci = ss.colIndex;
    const dir = ss.dir === "asc" ? 1 : -1;
    sorted.sort((a, b) => {
      const va = a[ci];
      const vb = b[ci];
      // NULL always last
      if ((va === null || va === undefined) && (vb === null || vb === undefined)) return 0;
      if (va === null || va === undefined) return 1;
      if (vb === null || vb === undefined) return -1;
      // EMPTY before NULL but after real values — empty goes toward end
      if (va === "" && vb === "") return 0;
      if (va === "") return 1;
      if (vb === "") return -1;
      // Numeric comparison
      if (typeof va === "number" && typeof vb === "number") return (va - vb) * dir;
      const na = Number(va);
      const nb = Number(vb);
      if (!isNaN(na) && !isNaN(nb) && va !== "" && vb !== "") return (na - nb) * dir;
      // String comparison
      return String(va).localeCompare(String(vb)) * dir;
    });
    return sorted;
  }

  function rebuildTbody() {
    tbody.innerHTML = "";
    const sorted = getSortedRows();
    sorted.forEach((row) => {
      const tr = document.createElement("tr");
      columns.forEach((_, i) => {
        const td = document.createElement("td");
        const val = row[i];
        if (val === null || val === undefined) {
          td.classList.add("null-value");
          td.textContent = "NULL";
        } else if (val === "") {
          td.classList.add("null-value");
          td.textContent = "EMPTY";
        } else {
          if (typeof val === "number") {
            td.style.textAlign = "right";
          }
          td.textContent = String(val);
        }
        tr.appendChild(td);
      });
      if (onRowClick) {
        tr.addEventListener("click", () => onRowClick(columns, row));
      }
      tbody.appendChild(tr);
    });
  }

  rebuildTbody();

  scrollDiv.appendChild(table);
  container.appendChild(scrollDiv);
}

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

    el.addEventListener("click", () => this.activate(id));
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
  },

  closeTab(id) {
    const idx = this.tabs.findIndex((t) => t.id === id);
    if (idx === -1) return;

    const tab = this.tabs[idx];
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
  },

  removeAll() {
    this.tabs.forEach((t) => {
      t.el.remove();
      t.paneEl.remove();
      delete sqlTabActions[t.id];
    });
    this.tabs = [];
    this.activeId = null;
    sqlTabCounter = 0;
  },

  has(id) {
    return this.tabs.some((t) => t.id === id);
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
  await showExplorer();
}

// ── Explorer ──

async function showExplorer() {
  explorerEl.classList.remove("hidden");
  sidebarDbName.textContent = currentDb;
  await loadTableList();
  const drafts = loadDrafts();
  if (drafts.length > 0) {
    drafts.forEach((text) => addSqlTab(text));
  } else {
    addSqlTab();
  }
}

async function loadTableList() {
  tableListEl.innerHTML = "";
  const loadingEl = document.createElement("div");
  loadingEl.className = "result-info";
  loadingEl.textContent = t('loading');
  tableListEl.appendChild(loadingEl);
  try {
    const res = await runQuery("SHOW TABLES");
    tableListEl.innerHTML = "";
    const tables = res.rows.map((r) => r[0]);
    currentTables = tables;

    tables.forEach((name) => {
      const el = document.createElement("div");
      el.className = "table-list-item";
      el.innerHTML = icon('table', 14);
      el.appendChild(document.createTextNode(' ' + name));

      el.addEventListener("click", () => openDataTab(name));

      el.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        e.stopPropagation();
        showContextMenu(e, [
          { label: t('data'), icon: "table", action: () => openDataTab(name) },
          { label: t('schema'), icon: "columns-3", action: () => openSchemaTab(name) },
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

// ── Data tab ──

function openDataTab(tableName) {
  const tabId = "data-" + tableName;

  tabManager.addTab(tabId, "data", tableName, (pane) => {
    const body = document.createElement("div");
    body.className = "data-tab-body";

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

    let currentPage = 0;
    let pageSize = 100;
    let totalRows = 0;
    let lastColumns = [];
    let lastRows = [];
    const sortState = { colIndex: -1, dir: null };

    // Column metadata from INFORMATION_SCHEMA
    let allColumns = [];    // [{name, dataType, columnKey}]
    let blobCols = [];      // column names
    let textCols = [];      // column names
    let pkCols = [];        // column names
    let truncateMode = true;
    let hasTruncatable = false;

    async function detectColumns() {
      const sql = "SELECT COLUMN_NAME, DATA_TYPE, COLUMN_KEY " +
        "FROM INFORMATION_SCHEMA.COLUMNS " +
        "WHERE TABLE_SCHEMA = '" + currentDb.replace(/'/g, "\\'") + "' " +
        "AND TABLE_NAME = '" + tableName.replace(/'/g, "\\'") + "' " +
        "ORDER BY ORDINAL_POSITION";
      const res = await runQuery(sql);
      allColumns = res.rows.map((r) => ({ name: r[0], dataType: (r[1] || "").toLowerCase(), columnKey: r[2] || "" }));
      const blobTypes = ["blob", "tinyblob", "mediumblob", "longblob", "binary", "varbinary"];
      const textTypes = ["text", "tinytext", "mediumtext", "longtext"];
      blobCols = allColumns.filter((c) => blobTypes.includes(c.dataType)).map((c) => c.name);
      textCols = allColumns.filter((c) => textTypes.includes(c.dataType)).map((c) => c.name);
      pkCols = allColumns.filter((c) => c.columnKey === "PRI").map((c) => c.name);
      hasTruncatable = blobCols.length > 0 || textCols.length > 0;
    }

    function buildSelectExpr() {
      if (!truncateMode || !hasTruncatable) return "*";
      return allColumns.map((c) => {
        const q = quoteId(c.name);
        if (blobCols.includes(c.name)) {
          return "'(BLOB)' AS " + q;
        }
        if (textCols.includes(c.name)) {
          return "CASE WHEN CHAR_LENGTH(" + q + ") > 200 THEN CONCAT(LEFT(" + q + ", 200), '\u2026') ELSE " + q + " END AS " + q;
        }
        return q;
      }).join(", ");
    }

    function onRowClick(columns, row) {
      if (pkCols.length > 0) {
        // Build WHERE clause from PK values in current row
        const whereParts = pkCols.map((pk) => {
          const idx = columns.indexOf(pk);
          if (idx === -1) return null;
          const val = row[idx];
          if (val === null || val === undefined) return quoteId(pk) + " IS NULL";
          if (typeof val === "number") return quoteId(pk) + " = " + val;
          return quoteId(pk) + " = '" + String(val).replace(/'/g, "\\'") + "'";
        }).filter(Boolean);

        if (whereParts.length > 0) {
          // Fetch full row but keep BLOB columns as placeholder (not worth transferring)
          const detailCols = allColumns.map((c) => {
            if (blobCols.includes(c.name)) return "'(BLOB)' AS " + quoteId(c.name);
            return quoteId(c.name);
          }).join(", ");
          runQuery("SELECT " + detailCols + " FROM " + quoteId(tableName) + " WHERE " + whereParts.join(" AND ") + " LIMIT 1")
            .then((res) => {
              if (res.rows.length > 0) {
                showRowDetailModal(res.columns, res.rows[0]);
              } else {
                showRowDetailModal(columns, row);
              }
            })
            .catch(() => showRowDetailModal(columns, row));
          return;
        }
      }
      showRowDetailModal(columns, row);
    }

    async function loadPage() {
      info.textContent = t('loading');
      tableContainer.innerHTML = "";
      try {
        const offset = currentPage * pageSize;
        const selectExpr = buildSelectExpr();
        const res = await runQuery(
          "SELECT " + selectExpr + " FROM " + quoteId(tableName) + " LIMIT " + offset + ", " + pageSize
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
        renderTable(res.columns, res.rows, tableContainer, onRowClick, sortState);
        renderFooter();
      } catch (error) {
        info.textContent = String(error);
      }
    }

    function renderFooter() {
      footerBar.innerHTML = "";
      const totalPages = Math.max(1, Math.ceil(totalRows / pageSize));

      // Paging nav
      const nav = document.createElement("div");
      nav.className = "paging-nav";

      const prevBtn = document.createElement("button");
      prevBtn.className = "ghost paging-btn";
      prevBtn.innerHTML = icon('chevron-left') + t('prev');
      prevBtn.disabled = currentPage === 0;
      prevBtn.addEventListener("click", () => { currentPage--; loadPage(); });
      nav.appendChild(prevBtn);

      const pageInfo = document.createElement("span");
      pageInfo.className = "paging-info";
      pageInfo.textContent = (currentPage + 1) + " / " + totalPages;
      nav.appendChild(pageInfo);

      const nextBtn = document.createElement("button");
      nextBtn.className = "ghost paging-btn";
      nextBtn.innerHTML = t('next') + icon('chevron-right');
      nextBtn.disabled = currentPage >= totalPages - 1;
      nextBtn.addEventListener("click", () => { currentPage++; loadPage(); });
      nav.appendChild(nextBtn);

      footerBar.appendChild(nav);

      // Page size selector
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
          loadPage();
        });
        sizeSelector.appendChild(btn);
      });
      footerBar.appendChild(sizeSelector);

      // Actions: Truncate + Schema + Export
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
          loadPage();
        });
        actions.appendChild(truncBtn);
      }

      const schemaBtn = document.createElement("button");
      schemaBtn.className = "ghost paging-btn";
      schemaBtn.innerHTML = icon('columns-3') + t('schema');
      schemaBtn.addEventListener("click", () => openSchemaTab(tableName));
      actions.appendChild(schemaBtn);

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

    Promise.all([
      runQuery("SELECT COUNT(*) FROM " + quoteId(tableName)),
      detectColumns(),
    ])
      .then(([countRes]) => {
        totalRows = countRes.rows[0][0];
        return loadPage();
      })
      .catch((error) => {
        info.textContent = String(error);
      });
  });
}

// ── Schema tab ──

function openSchemaTab(tableName) {
  const tabId = "schema-" + tableName;

  tabManager.addTab(tabId, "schema", tableName + " " + t('schema_suffix'), (pane) => {
    const body = document.createElement("div");
    body.className = "data-tab-body";

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

    // Footer with Data button
    const actions = document.createElement("div");
    actions.className = "footer-actions";
    const dataBtn = document.createElement("button");
    dataBtn.className = "ghost paging-btn";
    dataBtn.innerHTML = icon('table') + t('data');
    dataBtn.addEventListener("click", () => openDataTab(tableName));
    actions.appendChild(dataBtn);
    footerBar.appendChild(actions);

    runQuery("DESCRIBE " + quoteId(tableName))
      .then((res) => {
        info.textContent = t('n_columns', { n: res.rows.length });
        renderTable(res.columns, res.rows, tableContainer, (cols, row) => showRowDetailModal(cols, row));
      })
      .catch((error) => {
        info.textContent = String(error);
      });
  });
}

// ── SQL tab ──

function addSqlTab(initialContent) {
  sqlTabCounter++;
  const tabId = "sql-" + sqlTabCounter;
  const title = "SQL" + (sqlTabCounter > 1 ? " " + sqlTabCounter : "");

  tabManager.addTab(tabId, "sql", title, (pane) => {
    const inputArea = document.createElement("div");
    inputArea.className = "sql-input-area sql-editor";

    const editorDiv = document.createElement("div");
    inputArea.appendChild(editorDiv);

    let executing = false;

    const editor = CodeMirror(editorDiv, {
      mode: "text/x-mysql",
      lineNumbers: true,
      indentWithTabs: true,
      smartIndent: true,
      lineWrapping: false,
      hintOptions: {
        completeSingle: false,
        tables: buildTableHints(),
      },
      extraKeys: {
        "Ctrl-Enter": function () {
          if (executing) return;
          const stmt = getStatementAtCursor();
          if (stmt) executeStatements([stmt]);
        },
        "Ctrl-Space": "autocomplete",
      },
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

    if (initialContent) editor.setValue(initialContent);
    editor.on("change", scheduleDraftSave);

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
      const items = history.map((entry) => {
        const d = new Date(entry.ts);
        const time = String(d.getHours()).padStart(2, "0") + ":" + String(d.getMinutes()).padStart(2, "0");
        const sqlPreview = entry.sql.replace(/\s+/g, " ");
        const label = time + "  " + (sqlPreview.length > 60 ? sqlPreview.substring(0, 60) + "\u2026" : sqlPreview);
        return { label, action: () => editor.setValue(entry.sql) };
      });
      showContextMenu(ev, items);
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
        const formatted = window.sqlFormatter.format(selection, { language: "mysql" });
        editor.replaceSelection(formatted);
      } else {
        const cursor = editor.getCursor();
        const formatted = window.sqlFormatter.format(editor.getValue(), { language: "mysql" });
        editor.setValue(formatted);
        editor.setCursor(cursor);
      }
    });
    actions.appendChild(formatBtn);

    let cancelled = false;

    const cancelBtn = document.createElement("button");
    cancelBtn.className = "danger";
    cancelBtn.innerHTML = icon('x-circle') + t('cancel');
    cancelBtn.style.display = "none";
    cancelBtn.addEventListener("click", () => {
      cancelBtn.disabled = true;
      cancelled = true;
      safeInvoke("cancel_query").catch(() => {});
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
      // MySQL error: "... near 'xxx' at line N"
      const m = /near\s+'([\s\S]*?)'\s+at\s+line\s+(\d+)/i.exec(errorMsg);
      if (!m) return;
      const nearText = m[1];
      const errorLine = parseInt(m[2], 10) - 1; // 0-indexed
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

    async function executeStatements(statements) {
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

      try {
        for (const sql of statements) {
          const res = await runQuery(sql);

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

            const info = document.createElement("div");
            info.className = "result-info";
            info.textContent = t('n_rows', { n: res.rows.length });
            resultArea.appendChild(info);

            const wrapper = document.createElement("div");
            resultArea.appendChild(wrapper);
            renderTable(res.columns, res.rows, wrapper, (cols, row) => showRowDetailModal(cols, row));
          } else {
            const info = document.createElement("div");
            info.className = "result-info";
            const prefix = multi ? (sql.length > 60 ? sql.substring(0, 60) + "\u2026" : sql) + " \u2192 " : "";
            info.textContent = prefix + t('affected_rows', { n: res.affected_rows != null ? res.affected_rows : 0 });
            resultArea.appendChild(info);
          }
        }
        loadingEl.remove();
        if (lastColumns.length > 0) {
          exportBtn.style.display = "";
        }
      } catch (error) {
        loadingEl.remove();
        const errEl = document.createElement("div");
        errEl.className = "result-info";
        errEl.style.color = "var(--danger)";
        errEl.textContent = cancelled ? t('query_cancelled') : String(error);
        resultArea.appendChild(errEl);
        if (!cancelled) markSqlError(String(error));
      } finally {
        setExecuting(false);
      }
    }

    runLineBtn.addEventListener("click", () => {
      if (executing) return;
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
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") hideContextMenu();
});

// ── Reset ──

function resetExplorer() {
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
}

// ── Event wiring ──

dbSwitchBtn.addEventListener("click", () => {
  saveDrafts();
  tabManager.removeAll();
  showDbModal();
});

tabAddBtn.addEventListener("click", () => addSqlTab());

// ── Tauri event ──

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

      // If profile already has a database set, auto-select it
      if (requestCache.mysql.database) {
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

  eventApi.listen("menu:action", (event) => {
    const activeId = tabManager.activeId;
    const actions = sqlTabActions[activeId];
    switch (event.payload) {
      case "new-sql-tab": addSqlTab(); break;
      case "switch-db": dbSwitchBtn.click(); break;
      case "run": if (actions) actions.runLine(); break;
      case "run-all": if (actions) actions.runAll(); break;
      case "cancel": if (actions) actions.cancel(); break;
      case "format": if (actions) actions.format(); break;
      case "theme-light": setTheme("light"); break;
      case "theme-dark": setTheme("dark"); break;
      case "lang-en": setLang("en"); break;
      case "lang-ja": setLang("ja"); break;
    }
  });
}

// Re-render on language change
window.addEventListener("musql:langchange", () => {
  document.getElementById("db-modal-heading").innerHTML = icon('database', 20) + ' ' + t('select_database');
  applyI18n();
});
