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

// ── Image preview (#137) ──
// A value that holds a PNG, JPEG, GIF or WebP image is shown as the image. Bytes reach the
// page in one of three spellings, each reversible: `0x` + hex (MySQL and SQLite bytes that
// are not UTF-8, `bytes_to_json` in Rust), Base64 (a text column, bare or as a data URL),
// and redis-cli's escapes (a Redis value, `redis_client::escape`). The image goes into an
// <img> as a data URL, never as HTML. SVG is left out: it is text, and a guess at it would
// be loose.

// The largest image shown, in bytes. The query window's row detail fetches a BLOB only up
// to this size (`detailBlobExpr`), and says "(image: N bytes)" for a larger one.
const IMAGE_PREVIEW_MAX_BYTES = 5 * 1024 * 1024;
const IMAGE_TOO_LARGE_MARK = /^\(image: (\d+) bytes\)$/;

// The image type of the bytes that start a value, or null.
function imageMime(head) {
  const at = (i, ...bytes) => bytes.every((b, j) => head[i + j] === b);
  if (at(0, 0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a)) return "image/png";
  if (at(0, 0xff, 0xd8, 0xff)) return "image/jpeg";
  if (at(0, 0x47, 0x49, 0x46, 0x38)) return "image/gif"; // GIF8(7|9)a
  if (at(0, 0x52, 0x49, 0x46, 0x46) && at(8, 0x57, 0x45, 0x42, 0x50)) return "image/webp"; // RIFF....WEBP
  return null;
}

function bytesToBase64(bytes) {
  let binary = "";
  // In slices: String.fromCharCode takes its arguments on the stack.
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

// Hex digits as bytes. The caller checks the digits first (an image's megabytes of hex go
// through here, so no per-byte substring or parseInt).
function hexToBytes(hex) {
  const bytes = new Uint8Array(hex.length >> 1);
  const nibble = (c) => (c <= 0x39 ? c - 0x30 : (c | 0x20) - 0x57); // 0-9, a-f / A-F
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = (nibble(hex.charCodeAt(2 * i)) << 4) | nibble(hex.charCodeAt(2 * i + 1));
  }
  return bytes;
}

function base64Head(b64) {
  try {
    return Uint8Array.from(atob(b64.slice(0, 16)), (c) => c.charCodeAt(0));
  } catch (_) {
    return new Uint8Array(0);
  }
}

// redis-cli's escapes undone (`redis_client::escape` writes `\\`, `\"`, `\n`, `\r`, `\t`,
// `\a`, `\b` and `\xHH`; any other character is itself, in UTF-8).
function unescapeRedis(text) {
  const out = [];
  const encoder = new TextEncoder();
  const simple = { "\\": 0x5c, '"': 0x22, n: 0x0a, r: 0x0d, t: 0x09, a: 0x07, b: 0x08 };
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (c === "\\" && i + 1 < text.length) {
      const next = text[i + 1];
      if (next === "x" && /^[0-9a-fA-F]{2}$/.test(text.substr(i + 2, 2))) {
        out.push(parseInt(text.substr(i + 2, 2), 16));
        i += 3;
        continue;
      }
      if (Object.hasOwn(simple, next)) {
        out.push(simple[next]);
        i += 1;
        continue;
      }
    }
    const cp = text.codePointAt(i);
    if (cp > 0xffff) i++; // the other half of a surrogate pair
    for (const b of encoder.encode(String.fromCodePoint(cp))) out.push(b);
  }
  return Uint8Array.from(out);
}

// How the image starts in redis-cli's escapes: a value that does not start so is not
// unescaped (it may be plain text with backslashes of its own).
const REDIS_IMAGE_STARTS = ["\\x89PNG\\r\\n\\x1a\\n", "\\xff\\xd8\\xff", "GIF8", "RIFF"];

// Whether `val` starts as an image, reading only its start: { mime, spelling, body } or
// null. A value cut short (a Redis value over 1 MiB) can still be told apart this way.
// `redis`: the value may be in redis-cli's escapes.
function imageHead(val, redis) {
  if (typeof val !== "string" || val.length < 8) return null;
  if (val.startsWith("0x")) {
    const mime = imageMime(hexToBytes(val.slice(2, 26)));
    return mime ? { mime, spelling: "hex", body: val.slice(2) } : null;
  }
  const dataUrl = /^data:image\/(png|jpeg|gif|webp);base64,/i.exec(val);
  const body = dataUrl ? val.slice(dataUrl[0].length) : val;
  const mime = imageMime(base64Head(body.slice(0, 64).replace(/\s+/g, "")));
  if (mime) return { mime, spelling: "base64", body };
  if (redis && REDIS_IMAGE_STARTS.some((s) => val.startsWith(s))) {
    const redisMime = imageMime(unescapeRedis(val.slice(0, 64)));
    if (redisMime) return { mime: redisMime, spelling: "redis", body: val };
  }
  return null;
}

// What to show for `val`: { mime, bytes, dataUrl() } for an image, { tooLarge: bytes } for
// one over IMAGE_PREVIEW_MAX_BYTES, or null. The data URL is built only when the image is
// shown.
function imagePreview(val, redis) {
  if (typeof val !== "string") return null;
  const tooLarge = IMAGE_TOO_LARGE_MARK.exec(val);
  if (tooLarge) return { tooLarge: Number(tooLarge[1]) };
  const head = imageHead(val, redis);
  if (!head) return null;

  let bytes;
  let base64;
  if (head.spelling === "hex") {
    if (head.body.length % 2 !== 0 || !/^[0-9a-fA-F]*$/.test(head.body)) return null;
    bytes = head.body.length / 2;
    base64 = () => bytesToBase64(hexToBytes(head.body));
  } else if (head.spelling === "base64") {
    const b64 = head.body.replace(/\s+/g, "");
    if (b64.length % 4 !== 0 || !/^[A-Za-z0-9+/]+={0,2}$/.test(b64)) return null;
    bytes = (b64.length / 4) * 3 - /=*$/.exec(b64)[0].length;
    base64 = () => b64;
  } else {
    const raw = unescapeRedis(head.body);
    bytes = raw.length;
    base64 = () => bytesToBase64(raw);
  }
  if (bytes > IMAGE_PREVIEW_MAX_BYTES) return { tooLarge: bytes };
  return { mime: head.mime, bytes, dataUrl: () => "data:" + head.mime + ";base64," + base64() };
}

function formatBytes(n) {
  if (n < 1024) return n + " B";
  if (n < 1024 * 1024) return (n / 1024).toFixed(1) + " KB";
  return (n / (1024 * 1024)).toFixed(1) + " MB";
}

// Shows `preview` (from imagePreview) in `container`, with a button that switches to the
// value as text. `showText(target)` writes the text into `target`; it runs only when the
// text is asked for (a value of megabytes is slow to lay out), or when the image cannot
// be read (it only looked like one), in which case the text replaces the preview.
function renderImageValue(container, preview, showText) {
  const wrap = document.createElement("div");
  wrap.className = "value-image";
  const img = document.createElement("img");
  img.alt = "";
  const caption = document.createElement("div");
  caption.className = "value-image-caption";
  const label = (size) => [preview.mime.replace("image/", "").toUpperCase(), size, formatBytes(preview.bytes)]
    .filter(Boolean).join(", ");
  caption.textContent = label(null);
  const toggle = document.createElement("button");
  toggle.className = "ghost value-image-toggle";
  const textEl = document.createElement("div");
  textEl.className = "value-image-text hidden";
  let textShown = false;
  const setMode = (asText) => {
    if (asText && !textShown) {
      showText(textEl);
      textShown = true;
    }
    img.classList.toggle("hidden", asText);
    textEl.classList.toggle("hidden", !asText);
    toggle.textContent = asText ? t("image_show_image") : t("image_show_text");
  };
  toggle.addEventListener("click", () => setMode(img.classList.contains("hidden") === false));
  img.addEventListener("load", () => {
    caption.textContent = label(img.naturalWidth + "×" + img.naturalHeight);
  });
  img.addEventListener("error", () => {
    container.innerHTML = "";
    showText(container);
  });
  img.src = preview.dataUrl();
  wrap.append(img, textEl, caption, toggle);
  container.appendChild(wrap);
  setMode(false);
}

// A note under a value that is an image but is not shown (too large, or cut off).
function imageNote(text) {
  const note = document.createElement("div");
  note.className = "value-image-note";
  note.textContent = text;
  return note;
}

// ── Row detail modal ──

// `options.redis`: the values may be in redis-cli's escapes (`imagePreview`).
function showRowDetailModal(columns, row, options) {
  const redis = !!(options && options.redis);
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
      const showText = (target) => {
        const json = prettyJson(s);
        target.textContent = json ?? s;
        // JSON and long values get monospace for readability
        if (json !== null || s.length > 200) target.classList.add("monospace");
      };
      const preview = imagePreview(s, redis);
      if (preview && preview.dataUrl) {
        renderImageValue(valEl, preview, showText);
      } else {
        showText(valEl);
        if (preview && preview.tooLarge) {
          valEl.appendChild(imageNote(t("image_too_large", { size: formatBytes(preview.tooLarge) })));
        }
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
window.IMAGE_PREVIEW_MAX_BYTES = IMAGE_PREVIEW_MAX_BYTES;
window.imageHead = imageHead;
window.imagePreview = imagePreview;
window.renderImageValue = renderImageValue;
window.imageNote = imageNote;
window.formatBytes = formatBytes;
