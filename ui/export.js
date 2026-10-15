// Export shared by the connection windows (query, Redis #98): CSV / TSV of a result,
// and the options dialog (encoding, line ending, clipboard, #40). Needs i18n.js (`t`),
// icons.js (`icon`) and manual.js (`createHelpButton`).

function generateCsv(columns, rows, separator, newline) {
  const escapeField = (val) => {
    if (val === null || val === undefined) return "";
    const s = String(val);
    if (s.includes(separator) || s.includes('"') || s.includes("\n") || s.includes("\r")) {
      return '"' + s.replace(/"/g, '""') + '"';
    }
    return s;
  };
  const lines = [];
  lines.push(columns.map(escapeField).join(separator));
  rows.forEach((row) => {
    lines.push(columns.map((_, i) => escapeField(row[i])).join(separator));
  });
  // Only the record separator uses the chosen line ending; newlines embedded in
  // quoted field values are preserved verbatim (data fidelity — see #40 review).
  return lines.join(newline || "\n");
}

const EXPORT_ENCODINGS = [
  { value: "utf-8", label: "UTF-8" },
  { value: "utf-8-bom", label: "UTF-8 (BOM)" },
  { value: "shift_jis", label: "Shift_JIS" },
  { value: "euc-jp", label: "EUC-JP" },
];
const EXPORT_NEWLINES = [
  { value: "lf", label: "LF" },
  { value: "crlf", label: "CRLF" },
];

function getExportPref(key, fallback) {
  try { return localStorage.getItem("musql:export:" + key) || fallback; } catch (_) { return fallback; }
}
function setExportPref(key, val) {
  try { localStorage.setItem("musql:export:" + key, val); } catch (_) { /* ignore */ }
}

// Through the export_file command: the encoding is applied in Rust.
async function saveFile(content, defaultName, filterName, extensions, encoding) {
  return window.__TAURI__.core.invoke("export_file", {
    content, defaultName, filterName, extensions, encoding: encoding || "utf-8",
  });
}

// Opens the export options dialog. `generate` is an async (newline) => content that
// builds the payload using the chosen record separator; for file save the encoding is
// applied in Rust. The clipboard path writes Unicode text (encoding is a no-op there).
function openExportDialog({ defaultName, ext, filterName, generate }) {
  const overlay = document.createElement("div");
  overlay.className = "modal-overlay";
  const box = document.createElement("div");
  box.className = "modal-box";
  box.style.maxWidth = "360px";

  const h2 = document.createElement("h2");
  h2.textContent = t('export_options_title');
  const headingRow = document.createElement("div");
  headingRow.className = "modal-heading-row";
  headingRow.appendChild(h2);
  headingRow.appendChild(window.createHelpButton("export.md#文字コードと改行コード"));
  box.appendChild(headingRow);

  function makeSelect(labelText, options, current) {
    const label = document.createElement("label");
    const span = document.createElement("span");
    span.textContent = labelText;
    label.appendChild(span);
    const sel = document.createElement("select");
    options.forEach((o) => {
      const opt = document.createElement("option");
      opt.value = o.value;
      opt.textContent = o.label;
      sel.appendChild(opt);
    });
    sel.value = current;
    label.appendChild(sel);
    box.appendChild(label);
    return sel;
  }

  const encSel = makeSelect(t('export_charset'), EXPORT_ENCODINGS, getExportPref("encoding", "utf-8"));
  const nlSel = makeSelect(t('export_newline'), EXPORT_NEWLINES, getExportPref("newline", "lf"));

  const status = document.createElement("p");
  status.className = "modal-status";
  box.appendChild(status);

  const actions = document.createElement("div");
  actions.className = "actions actions-right";
  actions.style.marginTop = "16px";
  actions.style.flexWrap = "nowrap"; // keep Cancel / Copy / Save on one row
  const cancelBtn = document.createElement("button");
  cancelBtn.className = "ghost";
  cancelBtn.textContent = t('cancel');
  const copyBtn = document.createElement("button");
  copyBtn.className = "ghost";
  copyBtn.innerHTML = icon('copy', 14) + " " + t('export_copy_clipboard');
  const saveBtn = document.createElement("button");
  saveBtn.className = "success";
  saveBtn.innerHTML = icon('download', 14) + " " + t('export_save_file');
  actions.appendChild(cancelBtn);
  actions.appendChild(copyBtn);
  actions.appendChild(saveBtn);
  box.appendChild(actions);

  overlay.appendChild(box);
  document.body.appendChild(overlay);

  let busy = false;
  let closed = false;
  function close() {
    closed = true;
    overlay.remove();
    document.removeEventListener("keydown", onKey);
  }
  function onKey(e) {
    if (e.key === "Escape") close();
  }
  // Cancel / Escape / backdrop stay available even while generating so a slow
  // "all rows" export can always be dismissed; the `closed` guard then aborts the
  // in-flight save/copy so no OS dialog or write happens after dismissal (#40 review).
  function setBusy(b) {
    busy = b;
    [saveBtn, copyBtn, encSel, nlSel].forEach((el) => { el.disabled = b; });
  }
  function clearStatus() {
    status.textContent = "";
    status.style.color = "";
  }
  document.addEventListener("keydown", onKey);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
  cancelBtn.addEventListener("click", close);
  // A stale "copied" message must not imply the current selection was copied.
  [encSel, nlSel].forEach((sel) => sel.addEventListener("change", clearStatus));

  async function prepareContent() {
    setExportPref("newline", nlSel.value);
    const nl = nlSel.value === "crlf" ? "\r\n" : "\n";
    return generate(nl);
  }

  saveBtn.addEventListener("click", async () => {
    if (busy) return;
    setBusy(true);
    clearStatus();
    try {
      const content = await prepareContent();
      if (closed) return; // dismissed during generation → abort the save
      const enc = encSel.value;
      setExportPref("encoding", enc);
      const ok = await saveFile(content, defaultName, filterName, [ext], enc);
      if (ok) close(); else setBusy(false); // ok === false → user cancelled the OS save dialog
    } catch (err) {
      status.style.color = "var(--danger)";
      status.textContent = t('export_failed', { msg: String(err) });
      setBusy(false);
    }
  });

  copyBtn.addEventListener("click", async () => {
    if (busy) return;
    setBusy(true);
    clearStatus();
    try {
      // Encoding is a no-op for clipboard (Unicode text); only the line ending applies.
      const content = await prepareContent();
      if (closed) return; // dismissed during generation → abort the copy
      await navigator.clipboard.writeText(content);
      status.style.color = "var(--success)";
      status.textContent = t('export_copied');
      setBusy(false); // keep the dialog open so the user can copy again or save
    } catch (err) {
      status.style.color = "var(--danger)";
      status.textContent = t('export_failed', { msg: String(err) });
      setBusy(false);
    }
  });

  encSel.focus();
}

// The rows on screen as CSV (`sep` ",", `ext` "csv") or TSV ("\t", "tsv"). `name` (a
// table, a Redis key) becomes the default file name, with the characters Windows does
// not allow in one replaced.
function exportRows(columns, rows, name, sep, ext) {
  openExportDialog({
    defaultName: (name || "export").replace(/[\\/:*?"<>|]/g, "_") + "." + ext,
    ext,
    filterName: ext.toUpperCase(),
    generate: async (nl) => generateCsv(columns, rows, sep, nl),
  });
}

// The export menu's items for the rows on screen (CSV, TSV). A window adds its own
// items after these (query: the whole table, SQL, Markdown).
function currentRowsExportItems(columns, rows, name) {
  return [
    { label: t('csv_current'), icon: "download", action: () => exportRows(columns, rows, name, ",", "csv") },
    { label: t('tsv_current'), icon: "download", action: () => exportRows(columns, rows, name, "\t", "tsv") },
  ];
}

window.generateCsv = generateCsv;
window.openExportDialog = openExportDialog;
window.exportRows = exportRows;
window.currentRowsExportItems = currentRowsExportItems;
