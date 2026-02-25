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

// ── State ──
let requestCache = null;
let currentDb = null;
let sqlTabCounter = 0;
let currentTables = [];
let currentProfileId = null;

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

function sqlEscapeValue(val) {
  if (val === null || val === undefined) return "NULL";
  if (typeof val === "number") return String(val);
  return "'" + String(val).replace(/\\/g, "\\\\").replace(/'/g, "\\'") + "'";
}

async function generateInsertSql(tableName) {
  let createSql = "";
  try {
    const res = await runQuery("SHOW CREATE TABLE `" + tableName + "`");
    if (res.rows.length > 0) createSql = res.rows[0][1];
  } catch (_) { /* ignore */ }

  const dataRes = await runQuery("SELECT * FROM `" + tableName + "`", 0);
  const columns = dataRes.columns;
  const rows = dataRes.rows;

  const lines = [];
  lines.push("-- mysqldump-compatible export");
  lines.push("-- Table: " + tableName);
  lines.push("-- Exported: " + new Date().toISOString());
  lines.push("");
  lines.push("DROP TABLE IF EXISTS `" + tableName + "`;");
  lines.push("");
  if (createSql) {
    lines.push(createSql + ";");
    lines.push("");
  }
  if (rows.length > 0) {
    const colList = columns.map((c) => "`" + c + "`").join(", ");
    for (let i = 0; i < rows.length; i += 1000) {
      const batch = rows.slice(i, i + 1000);
      lines.push("INSERT INTO `" + tableName + "` (" + colList + ") VALUES");
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
    { label: "CSV (current)", action: () => doExportCurrent(columns, rows, tableName, ",", "csv") },
    { label: "TSV (current)", action: () => doExportCurrent(columns, rows, tableName, "\t", "tsv") },
  ];
  if (tableName) {
    items.push({ separator: true });
    items.push({ label: "CSV (all rows)", action: () => doExportAll(tableName, ",", "csv") });
    items.push({ label: "TSV (all rows)", action: () => doExportAll(tableName, "\t", "tsv") });
    items.push({ label: "SQL (all rows)", action: () => doExportSql(tableName) });
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
  const res = await runQuery("SELECT * FROM `" + tableName + "`", 0);
  const content = generateCsv(res.columns, res.rows, sep);
  await saveFile(content, tableName + "_all." + ext, ext.toUpperCase(), [ext]);
}

async function doExportSql(tableName) {
  const content = await generateInsertSql(tableName);
  await saveFile(content, tableName + ".sql", "SQL", ["sql"]);
}

// ── renderTable utility ──

function renderTable(columns, rows, container) {
  container.innerHTML = "";
  const scrollDiv = document.createElement("div");
  scrollDiv.className = "table-scroll";

  const table = document.createElement("table");
  table.className = "result-table";

  const thead = document.createElement("thead");
  const headerRow = document.createElement("tr");
  columns.forEach((col) => {
    const th = document.createElement("th");
    th.textContent = col;
    headerRow.appendChild(th);
  });
  thead.appendChild(headerRow);
  table.appendChild(thead);

  const tbody = document.createElement("tbody");
  rows.forEach((row) => {
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
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);

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

    const labelSpan = document.createElement("span");
    labelSpan.textContent = title;
    el.appendChild(labelSpan);

    const closeBtn = document.createElement("span");
    closeBtn.className = "tab-close";
    closeBtn.textContent = "\u00D7";
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
  dbModalStatus.textContent = "Loading...";

  try {
    const res = await runQuery("SHOW DATABASES");
    dbModalStatus.textContent = "";
    const dbNames = res.rows.map((r) => r[0]);

    dbNames.forEach((name) => {
      const el = document.createElement("div");
      el.className = "db-list-item";
      el.textContent = name;
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
  loadingEl.textContent = "Loading...";
  tableListEl.appendChild(loadingEl);
  try {
    const res = await runQuery("SHOW TABLES");
    tableListEl.innerHTML = "";
    const tables = res.rows.map((r) => r[0]);
    currentTables = tables;

    tables.forEach((name) => {
      const el = document.createElement("div");
      el.className = "table-list-item";
      el.textContent = name;

      el.addEventListener("click", () => openDataTab(name));

      el.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        e.stopPropagation();
        showContextMenu(e, [
          { label: "Data", action: () => openDataTab(name) },
          { label: "Schema", action: () => openSchemaTab(name) },
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
    info.textContent = "Loading...";
    body.appendChild(info);

    const tableContainer = document.createElement("div");
    tableContainer.className = "table-scroll";
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

    async function loadPage() {
      info.textContent = "Loading...";
      tableContainer.innerHTML = "";
      try {
        const offset = currentPage * pageSize;
        const res = await runQuery(
          "SELECT * FROM `" + tableName + "` LIMIT " + offset + ", " + pageSize
        );
        lastColumns = res.columns;
        lastRows = res.rows;
        if (totalRows === 0 && res.rows.length === 0) {
          info.textContent = "0 rows";
        } else {
          const from = offset + 1;
          const to = offset + res.rows.length;
          info.textContent = "Rows " + from + "\u2013" + to + " of " + totalRows;
        }
        renderTable(res.columns, res.rows, tableContainer);
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
      prevBtn.textContent = "\u2039 Prev";
      prevBtn.disabled = currentPage === 0;
      prevBtn.addEventListener("click", () => { currentPage--; loadPage(); });
      nav.appendChild(prevBtn);

      const pageInfo = document.createElement("span");
      pageInfo.className = "paging-info";
      pageInfo.textContent = (currentPage + 1) + " / " + totalPages;
      nav.appendChild(pageInfo);

      const nextBtn = document.createElement("button");
      nextBtn.className = "ghost paging-btn";
      nextBtn.textContent = "Next \u203A";
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

      // Actions: Schema + Export
      const actions = document.createElement("div");
      actions.className = "footer-actions";

      const schemaBtn = document.createElement("button");
      schemaBtn.className = "ghost paging-btn";
      schemaBtn.textContent = "Schema";
      schemaBtn.addEventListener("click", () => openSchemaTab(tableName));
      actions.appendChild(schemaBtn);

      const exportBtn = document.createElement("button");
      exportBtn.className = "ghost paging-btn";
      exportBtn.textContent = "Export \u25BE";
      exportBtn.addEventListener("click", (ev) => {
        ev.stopPropagation();
        showExportMenu(ev, lastColumns, lastRows, tableName);
      });
      actions.appendChild(exportBtn);

      footerBar.appendChild(actions);
    }

    runQuery("SELECT COUNT(*) FROM `" + tableName + "`")
      .then((res) => {
        totalRows = res.rows[0][0];
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

  tabManager.addTab(tabId, "schema", tableName + " (schema)", (pane) => {
    const body = document.createElement("div");
    body.className = "data-tab-body";

    const info = document.createElement("div");
    info.className = "result-info";
    info.textContent = "Loading...";
    body.appendChild(info);

    const tableContainer = document.createElement("div");
    tableContainer.className = "table-scroll";
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
    dataBtn.textContent = "Data";
    dataBtn.addEventListener("click", () => openDataTab(tableName));
    actions.appendChild(dataBtn);
    footerBar.appendChild(actions);

    runQuery("DESCRIBE `" + tableName + "`")
      .then((res) => {
        info.textContent = res.rows.length + " columns";
        renderTable(res.columns, res.rows, tableContainer);
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
    historyBtn.textContent = "History \u25BE";
    historyBtn.addEventListener("click", (ev) => {
      ev.stopPropagation();
      const history = loadHistory();
      if (history.length === 0) {
        showContextMenu(ev, [{ label: "(no history)", action: () => {} }]);
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

    const runLineBtn = document.createElement("button");
    runLineBtn.className = "ghost";
    runLineBtn.textContent = "Run this line";
    actions.appendChild(runLineBtn);

    const runAllBtn = document.createElement("button");
    runAllBtn.textContent = "Run all";
    actions.appendChild(runAllBtn);

    const exportBtn = document.createElement("button");
    exportBtn.className = "ghost";
    exportBtn.textContent = "Export \u25BE";
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

    async function executeStatements(statements) {
      resultArea.innerHTML = "";
      lastColumns = [];
      lastRows = [];
      exportBtn.style.display = "none";
      clearErrorMarks();
      if (statements.length === 0) return;

      const multi = statements.length > 1;
      const loadingEl = document.createElement("div");
      loadingEl.className = "result-info";
      loadingEl.textContent = "Running...";
      resultArea.appendChild(loadingEl);

      try {
        for (const sql of statements) {
          const res = await runQuery(sql);
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
            info.textContent = res.rows.length + " rows";
            resultArea.appendChild(info);

            const wrapper = document.createElement("div");
            resultArea.appendChild(wrapper);
            renderTable(res.columns, res.rows, wrapper);
          } else {
            const info = document.createElement("div");
            info.className = "result-info";
            const prefix = multi ? (sql.length > 60 ? sql.substring(0, 60) + "\u2026" : sql) + " \u2192 " : "";
            info.textContent = prefix + "Affected rows: " + (res.affected_rows != null ? res.affected_rows : 0);
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
        errEl.style.color = "#d24a4a";
        errEl.textContent = String(error);
        resultArea.appendChild(errEl);
        markSqlError(String(error));
      }
    }

    runLineBtn.addEventListener("click", () => {
      const stmt = getStatementAtCursor();
      if (stmt) executeStatements([stmt]);
    });

    runAllBtn.addEventListener("click", () => {
      const stmts = getAllStatements();
      if (stmts.length > 0) executeStatements(stmts);
    });

    exportBtn.addEventListener("click", (ev) => {
      ev.stopPropagation();
      if (lastColumns.length === 0) return;
      showContextMenu(ev, [
        { label: "CSV", action: () => doExportCurrent(lastColumns, lastRows, null, ",", "csv") },
        { label: "TSV", action: () => doExportCurrent(lastColumns, lastRows, null, "\t", "tsv") },
      ]);
    });
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
    el.textContent = mi.label;
    if (mi.danger) el.style.color = "#d24a4a";
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
  requestCache = null;
  explorerEl.classList.add("hidden");
  dbModal.classList.add("hidden");
  tableListEl.innerHTML = "";
  sidebarDbName.textContent = "";
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
}
