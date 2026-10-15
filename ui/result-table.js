// Result tables shared by the windows that show rows: the query window (SQL results,
// table data) and the Redis window (hash / list / set / zset / stream values, #98).
// Needs i18n.js (`t`) and icons.js (`icon`) to be loaded first.

// A JSON object or array formatted for reading, or null when `s` is not one.
function prettyJson(s) {
  const trimmed = s.trim();
  if (!((trimmed.startsWith("{") || trimmed.startsWith("[")) && trimmed.length > 2)) return null;
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch (_) {
    return null;
  }
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
      const json = prettyJson(s);
      valEl.textContent = json ?? s;
      if (json !== null) valEl.classList.add("monospace");
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

// The column header's sort marks (black up / down-pointing triangle), built from code
// points so the script does not depend on how the file is decoded.
const SORT_ASC_MARK = " " + String.fromCharCode(0x25b2);
const SORT_DESC_MARK = " " + String.fromCharCode(0x25bc);

function renderTable(columns, rows, container, onRowClick, sortState, onSortCallback) {
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
    if (ci === ss.colIndex && ss.dir === "asc") { indicator.textContent = SORT_ASC_MARK; th.classList.add("sort-active"); }
    else if (ci === ss.colIndex && ss.dir === "desc") { indicator.textContent = SORT_DESC_MARK; th.classList.add("sort-active"); }
    th.appendChild(indicator);

    th.addEventListener("click", () => {
      if (onSortCallback) {
        // Server-side sort: delegate to callback
        onSortCallback(ci);
        return;
      }
      // Client-side sort
      if (ss.colIndex === ci) {
        if (ss.dir === "asc") ss.dir = "desc";
        else if (ss.dir === "desc") { ss.dir = null; ss.colIndex = -1; }
        else ss.dir = "asc";
      } else {
        ss.colIndex = ci;
        ss.dir = "asc";
      }
      thEls.forEach((headerCell, ti) => {
        const ind = headerCell.querySelector(".sort-indicator");
        headerCell.classList.toggle("sort-active", ti === ss.colIndex && ss.dir !== null);
        if (ti === ss.colIndex && ss.dir === "asc") ind.textContent = SORT_ASC_MARK;
        else if (ti === ss.colIndex && ss.dir === "desc") ind.textContent = SORT_DESC_MARK;
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
    if (onSortCallback || ss.colIndex < 0 || ss.dir === null) return rows;
    return sortRowsByColumn(rows, ss.colIndex, ss.dir);
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

// Client-side row sort: NULL and EMPTY last, numbers by value, the rest by text.
function sortRowsByColumn(rows, ci, dir) {
  const mult = dir === "asc" ? 1 : -1;
  const sorted = rows.slice();
  sorted.sort((a, b) => {
    const va = a[ci];
    const vb = b[ci];
    if ((va === null || va === undefined) && (vb === null || vb === undefined)) return 0;
    if (va === null || va === undefined) return 1;
    if (vb === null || vb === undefined) return -1;
    if (va === "" && vb === "") return 0;
    if (va === "") return 1;
    if (vb === "") return -1;
    if (typeof va === "number" && typeof vb === "number") return (va - vb) * mult;
    const na = Number(va);
    const nb = Number(vb);
    if (!Number.isNaN(na) && !Number.isNaN(nb) && va !== "" && vb !== "") return (na - nb) * mult;
    return String(va).localeCompare(String(vb)) * mult;
  });
  return sorted;
}

// Render a result set with client-side pagination + sort (used by the SQL tab).
// The full row set is kept in memory; only the current page is rendered.
function renderPagedTable(columns, rows, container, onRowClick) {
  container.innerHTML = "";
  const tableWrap = document.createElement("div");
  const footer = document.createElement("div");
  footer.className = "paging-bar";
  container.appendChild(tableWrap);
  container.appendChild(footer);

  let page = 0;
  let pageSize = 100;
  const sortState = { colIndex: -1, dir: null };
  let viewRows = rows;

  function applySort() {
    viewRows = (sortState.colIndex < 0 || sortState.dir === null)
      ? rows
      : sortRowsByColumn(rows, sortState.colIndex, sortState.dir);
  }

  function onSort(ci) {
    if (sortState.colIndex === ci) {
      if (sortState.dir === "asc") sortState.dir = "desc";
      else if (sortState.dir === "desc") { sortState.dir = null; sortState.colIndex = -1; }
      else sortState.dir = "asc";
    } else {
      sortState.colIndex = ci;
      sortState.dir = "asc";
    }
    page = 0;
    applySort();
    render();
  }

  function renderFooter(totalPages, offset, count) {
    footer.innerHTML = "";

    const nav = document.createElement("div");
    nav.className = "paging-nav";
    const prevBtn = document.createElement("button");
    prevBtn.className = "ghost paging-btn";
    prevBtn.innerHTML = icon('chevron-left') + t('prev');
    prevBtn.disabled = page === 0;
    prevBtn.addEventListener("click", () => { page--; render(); });
    nav.appendChild(prevBtn);
    const pageInfo = document.createElement("span");
    pageInfo.className = "paging-info";
    pageInfo.textContent = (page + 1) + " / " + totalPages;
    nav.appendChild(pageInfo);
    const nextBtn = document.createElement("button");
    nextBtn.className = "ghost paging-btn";
    nextBtn.innerHTML = t('next') + icon('chevron-right');
    nextBtn.disabled = page >= totalPages - 1;
    nextBtn.addEventListener("click", () => { page++; render(); });
    nav.appendChild(nextBtn);
    footer.appendChild(nav);

    const sizeSelector = document.createElement("div");
    sizeSelector.className = "paging-sizes";
    [50, 100, 200, 500].forEach((size) => {
      const btn = document.createElement("button");
      btn.className = "ghost paging-size-btn" + (size === pageSize ? " active" : "");
      btn.textContent = size;
      btn.addEventListener("click", () => {
        if (size === pageSize) return;
        pageSize = size;
        page = 0;
        render();
      });
      sizeSelector.appendChild(btn);
    });
    footer.appendChild(sizeSelector);

    const actions = document.createElement("div");
    actions.className = "footer-actions";
    const range = document.createElement("span");
    range.className = "paging-info";
    range.textContent = viewRows.length === 0
      ? t('rows_range', { from: 0, to: 0, total: 0 })
      : t('rows_range', { from: offset + 1, to: offset + count, total: viewRows.length });
    actions.appendChild(range);
    footer.appendChild(actions);
  }

  function render() {
    const totalPages = Math.max(1, Math.ceil(viewRows.length / pageSize));
    if (page >= totalPages) page = totalPages - 1;
    if (page < 0) page = 0;
    const offset = page * pageSize;
    const slice = viewRows.slice(offset, offset + pageSize);
    renderTable(columns, slice, tableWrap, onRowClick, sortState, (ci) => onSort(ci));
    renderFooter(totalPages, offset, slice.length);
  }

  applySort();
  render();
}

// What the windows' scripts call.
window.prettyJson = prettyJson;
window.showRowDetailModal = showRowDetailModal;
window.renderTable = renderTable;
window.renderPagedTable = renderPagedTable;
