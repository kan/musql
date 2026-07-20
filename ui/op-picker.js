// 1Password secret-reference picker, shared by the settings window and the AI settings
// modal. Two steps: pick an item, then pick one of its fields. `op` hands back a ready
// made op://vault/item/field reference per field, so nothing is assembled here.
//
// window.openOpPicker() -> Promise<string|null>   (null = cancelled)
(function () {
  const MAX_RESULTS = 200;

  const tr = (key, fallback) =>
    typeof window.t === "function" ? window.t(key) || fallback : fallback;

  function invokeTauri(cmd, args) {
    if (window.__TAURI__ && window.__TAURI__.core) {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    return Promise.reject(new Error("Tauri API unavailable"));
  }

  // Subsequence scoring, same shape as the query window's QuickOpen: contiguous runs and
  // a match at position 0 score higher; a query that is not a subsequence scores -1.
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

  let overlay = null;
  let filterInput = null;
  let listEl = null;
  let statusEl = null;
  let titleEl = null;
  let backBtn = null;
  let refreshBtn = null;

  let items = [];
  let fields = [];
  let resolvePicker = null;
  let step = "items"; // "items" | "fields"
  let currentItem = null;
  let loading = false;

  function setStatus(message, ok) {
    statusEl.textContent = message || "";
    statusEl.style.color = ok === false ? "var(--danger)" : "var(--sub)";
  }

  // Spinner + text, built as nodes rather than innerHTML.
  function setBusyStatus(message) {
    statusEl.textContent = "";
    statusEl.style.color = "var(--sub)";
    const spinner = document.createElement("span");
    spinner.className = "spinner";
    statusEl.appendChild(spinner);
    statusEl.appendChild(document.createTextNode(message));
  }

  // Reload would start a second CLI call on top of the one in flight. "Back" stays live
  // so a slow field fetch can still be abandoned — the reply is discarded by the
  // currentItem guard in showFields.
  function setLoading(value) {
    loading = value;
    refreshBtn.disabled = value;
  }

  function finish(reference) {
    const resolve = resolvePicker;
    resolvePicker = null;
    overlay.classList.add("hidden");
    listEl.textContent = "";
    if (resolve) resolve(reference);
  }

  function buildDom() {
    overlay = document.createElement("div");
    overlay.className = "modal-overlay hidden";
    overlay.id = "op-picker-overlay";

    const box = document.createElement("div");
    box.className = "modal-box op-picker-box";

    const headingRow = document.createElement("div");
    headingRow.className = "modal-heading-row";
    titleEl = document.createElement("h2");
    headingRow.appendChild(titleEl);
    if (typeof window.createHelpButton === "function") {
      headingRow.appendChild(window.createHelpButton("1password.md#設定"));
    }
    box.appendChild(headingRow);

    filterInput = document.createElement("input");
    filterInput.type = "text";
    filterInput.id = "op-picker-filter";
    box.appendChild(filterInput);

    statusEl = document.createElement("p");
    statusEl.className = "modal-status";
    box.appendChild(statusEl);

    listEl = document.createElement("div");
    listEl.className = "op-picker-list";
    box.appendChild(listEl);

    const actions = document.createElement("div");
    actions.className = "actions actions-right";
    backBtn = document.createElement("button");
    backBtn.className = "ghost";
    backBtn.addEventListener("click", showItems);
    refreshBtn = document.createElement("button");
    refreshBtn.className = "ghost";
    refreshBtn.addEventListener("click", () => loadItems(true));
    const cancelBtn = document.createElement("button");
    cancelBtn.className = "ghost";
    cancelBtn.textContent = tr("cancel", "Cancel");
    cancelBtn.addEventListener("click", () => finish(null));
    actions.appendChild(backBtn);
    actions.appendChild(refreshBtn);
    actions.appendChild(cancelBtn);
    box.appendChild(actions);

    overlay.appendChild(box);
    document.body.appendChild(overlay);

    filterInput.addEventListener("input", render);
    overlay.addEventListener("click", (e) => {
      if (e.target === overlay) finish(null);
    });
    // On document, in the capture phase: clicking a row removes it from the DOM, which
    // drops focus to body, so a listener on the overlay would never see the key. Capture
    // also puts us ahead of the query window's own Escape handler, which would otherwise
    // close the modal behind this one.
    document.addEventListener(
      "keydown",
      (e) => {
        if (e.key !== "Escape" || !overlay || overlay.classList.contains("hidden")) return;
        e.preventDefault();
        e.stopPropagation();
        if (step === "fields") showItems();
        else finish(null);
      },
      true
    );
  }

  function applyLabels() {
    titleEl.textContent =
      step === "fields" ? tr("op_pick_field", "Select field") : tr("op_pick_item", "Select item");
    filterInput.placeholder =
      step === "fields" ? tr("op_filter_fields", "Filter fields…") : tr("op_filter_items", "Search items…");
    backBtn.textContent = tr("op_back_to_items", "Back");
    backBtn.classList.toggle("hidden", step !== "fields");
    refreshBtn.textContent = tr("op_refresh", "Reload");
    refreshBtn.classList.toggle("hidden", step !== "items");
  }

  function addRow(primary, secondary, onPick) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "op-picker-row";
    const main = document.createElement("span");
    main.className = "op-picker-row-title";
    main.textContent = primary;
    row.appendChild(main);
    if (secondary) {
      const sub = document.createElement("span");
      sub.className = "op-picker-row-sub";
      sub.textContent = secondary;
      row.appendChild(sub);
    }
    row.addEventListener("click", onPick);
    listEl.appendChild(row);
  }

  function render() {
    if (step === "fields") return renderFields();
    renderItems();
  }

  function renderItems() {
    const query = filterInput.value.trim();
    listEl.textContent = "";
    let matched;
    if (query) {
      const scored = [];
      for (const item of items) {
        const score = fuzzyScore(query, `${item.title} ${item.vault}`);
        if (score >= 0) scored.push({ item, score });
      }
      scored.sort((a, b) => b.score - a.score);
      matched = scored.map((s) => s.item);
    } else {
      // Already sorted by title in loadItems.
      matched = items;
    }
    const shown = matched.slice(0, MAX_RESULTS);
    for (const item of shown) {
      addRow(item.title || "(no title)", `${item.vault} · ${item.category}`, () =>
        showFields(item)
      );
    }
    if (matched.length === 0) {
      setStatus(tr("op_no_items", "No matching items"));
    } else if (matched.length > shown.length) {
      setStatus(
        tr("op_more_items", "Showing the first {n}. Narrow the search.").replace(
          "{n}",
          String(shown.length)
        )
      );
    } else {
      setStatus("");
    }
  }

  function renderFields() {
    const query = filterInput.value.trim();
    listEl.textContent = "";
    const matched = fields.filter((f) => fuzzyScore(query, f.label) >= 0);
    for (const field of matched) {
      addRow(field.label, field.kind, () => finish(field.reference));
    }
    setStatus(matched.length === 0 ? tr("op_no_fields", "No referenceable fields") : "");
  }

  function showItems() {
    step = "items";
    currentItem = null;
    filterInput.value = "";
    applyLabels();
    render();
    filterInput.focus();
  }

  async function showFields(item) {
    step = "fields";
    currentItem = item;
    fields = [];
    filterInput.value = "";
    applyLabels();
    listEl.textContent = "";
    // The clicked row is gone; without this, focus falls to body and typing does nothing.
    filterInput.focus();
    setLoading(true);
    setBusyStatus(tr("op_loading", "Loading from 1Password…"));
    try {
      const result = await invokeTauri("op_list_fields", { itemId: item.id });
      setLoading(false);
      // The user may have gone back or closed the picker while this was in flight.
      if (step !== "fields" || currentItem !== item) return;
      fields = result || [];
      render();
    } catch (e) {
      setLoading(false);
      if (step !== "fields" || currentItem !== item) return;
      setStatus(String(e), false);
    }
  }

  async function loadItems(refresh) {
    listEl.textContent = "";
    setLoading(true);
    // `op` cannot cache on Windows, so every list is a full round trip (~10s regardless
    // of how many items there are — filtering server-side does not help). Say so rather
    // than leave the user staring at a blank modal.
    setBusyStatus(tr("op_loading_items", "Loading items from 1Password…"));
    try {
      const result = (await invokeTauri("op_list_items", { refresh: !!refresh })) || [];
      setLoading(false);
      // A late reply must not write into a picker the user already dismissed.
      if (!resolvePicker || step !== "items") return;
      // Sorted once here so the empty query — where every fuzzy score ties — does not
      // pay a full localeCompare sort of ~1400 items on each render.
      result.sort((a, b) => (a.title || "").localeCompare(b.title || ""));
      items = result;
      render();
    } catch (e) {
      setLoading(false);
      if (!resolvePicker) return;
      setStatus(String(e), false);
    }
  }

  window.openOpPicker = function openOpPicker() {
    if (!overlay) buildDom();
    return new Promise((resolve) => {
      // A second call while one is open would strand the first promise.
      if (resolvePicker) resolvePicker(null);
      resolvePicker = resolve;
      step = "items";
      currentItem = null;
      filterInput.value = "";
      applyLabels();
      overlay.classList.remove("hidden");
      filterInput.focus();
      loadItems(false);
    });
  };

  window.addEventListener("musql:langchange", () => {
    if (!overlay || overlay.classList.contains("hidden")) return;
    applyLabels();
    // Re-rendering mid-fetch would replace the "loading" status with "no results".
    if (!loading) render();
  });
})();
