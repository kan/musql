// In-app manual viewer (#46).
// Fetches docs/manual/*.md from raw.githubusercontent.com (no bundling; the
// manual on the default branch is the single source of truth) and renders it
// with a small built-in Markdown renderer into a modal overlay.
// Entry points: window.openManual(page, anchor), elements with
// [data-manual="page.md#anchor"], and the F1 key.
(function () {
  const RAW_BASE = "https://raw.githubusercontent.com/kan/musql/main/docs/manual/";
  const BLOB_BASE = "https://github.com/kan/musql/blob/main/docs/manual/";
  const HOME_PAGE = "README.md";

  const cache = new Map(); // page -> markdown text
  const backStack = [];
  let overlay = null;
  let contentEl = null;
  let backBtn = null;
  let current = null; // current page name

  function tr(key, fallback) {
    return typeof window.t === "function" ? window.t(key) : fallback;
  }

  function invokeTauri(cmd, args) {
    if (window.__TAURI__ && window.__TAURI__.core) {
      return window.__TAURI__.core.invoke(cmd, args);
    }
    return Promise.reject(new Error("Tauri API unavailable"));
  }

  function openExternal(url) {
    invokeTauri("open_external", { url }).catch(() => {});
  }

  // Resolve a relative link against docs/manual/. Returns null when the path
  // escapes the manual directory (those open on GitHub instead).
  function resolveManualPath(href) {
    const parts = href.replace(/^\.\//, "").split("/");
    const out = [];
    for (const part of parts) {
      if (part === "" || part === ".") continue;
      if (part === "..") {
        if (out.length === 0) return null;
        out.pop();
      } else {
        out.push(part);
      }
    }
    return out.join("/");
  }

  // ── Markdown rendering ──

  // GitHub-style heading slug: lowercase, strip punctuation, spaces to "-".
  function makeSlugger() {
    const seen = new Map();
    return (text) => {
      let slug = text
        .toLowerCase()
        .replace(/[^\p{L}\p{N}\p{M}_\- ]/gu, "")
        .replace(/ /g, "-");
      const n = seen.get(slug);
      seen.set(slug, (n || 0) + 1);
      if (n) slug = `${slug}-${n}`;
      return slug;
    };
  }

  const INLINE_RE = /!\[([^\]]*)\]\(([^)\s]+)\)|\[([^\]]+)\]\(([^)\s]+)\)|`([^`]+)`|\*\*([^*]+)\*\*/g;

  function renderInline(text, page) {
    const frag = document.createDocumentFragment();
    let last = 0;
    for (const m of text.matchAll(INLINE_RE)) {
      if (m.index > last) frag.appendChild(document.createTextNode(text.slice(last, m.index)));
      last = m.index + m[0].length;
      if (m[1] !== undefined) {
        // image
        const img = document.createElement("img");
        img.alt = m[1];
        img.loading = "lazy";
        img.src = /^https?:/.test(m[2]) ? m[2] : RAW_BASE + (resolveManualPath(m[2]) || m[2]);
        frag.appendChild(img);
      } else if (m[3] !== undefined) {
        // link
        frag.appendChild(makeLink(m[3], m[4], page));
      } else if (m[5] !== undefined) {
        const code = document.createElement("code");
        code.textContent = m[5];
        frag.appendChild(code);
      } else {
        const strong = document.createElement("strong");
        strong.appendChild(renderInline(m[6], page));
        frag.appendChild(strong);
      }
    }
    if (last < text.length) frag.appendChild(document.createTextNode(text.slice(last)));
    return frag;
  }

  function makeLink(label, href, page) {
    const a = document.createElement("a");
    a.appendChild(renderInline(label, page));
    a.href = "#";
    if (href.startsWith("#")) {
      a.dataset.mdPage = current || page;
      a.dataset.mdAnchor = href.slice(1);
    } else if (/^https?:/.test(href)) {
      a.dataset.external = href;
    } else {
      const [path, anchor] = href.split("#");
      const resolved = /\.md$/.test(path) ? resolveManualPath(path) : null;
      if (resolved) {
        a.dataset.mdPage = resolved;
        if (anchor) a.dataset.mdAnchor = anchor;
      } else {
        a.dataset.external = BLOB_BASE + href;
      }
    }
    return a;
  }

  function stripInline(text) {
    return text
      .replace(/!\[([^\]]*)\]\([^)\s]+\)/g, "$1")
      .replace(/\[([^\]]+)\]\([^)\s]+\)/g, "$1")
      .replace(/`([^`]+)`/g, "$1")
      .replace(/\*\*([^*]+)\*\*/g, "$1");
  }

  function renderMarkdown(md, page) {
    const frag = document.createDocumentFragment();
    const slugger = makeSlugger();
    const lines = md.replace(/<!--[\s\S]*?-->/g, "").split(/\r?\n/);
    let i = 0;
    let para = [];

    const flushPara = () => {
      if (para.length === 0) return;
      const p = document.createElement("p");
      p.appendChild(renderInline(para.join(" "), page));
      frag.appendChild(p);
      para = [];
    };

    while (i < lines.length) {
      const line = lines[i];

      // fenced code block
      if (/^```/.test(line)) {
        flushPara();
        const buf = [];
        i++;
        while (i < lines.length && !/^```/.test(lines[i])) {
          buf.push(lines[i]);
          i++;
        }
        i++; // closing fence
        const pre = document.createElement("pre");
        const code = document.createElement("code");
        code.textContent = buf.join("\n");
        pre.appendChild(code);
        frag.appendChild(pre);
        continue;
      }

      // heading
      const hm = line.match(/^(#{1,6})\s+(.*)$/);
      if (hm) {
        flushPara();
        const h = document.createElement(`h${hm[1].length}`);
        h.id = slugger(stripInline(hm[2]));
        h.appendChild(renderInline(hm[2], page));
        frag.appendChild(h);
        i++;
        continue;
      }

      // table
      if (/^\s*\|/.test(line) && i + 1 < lines.length && /^\s*\|?[\s:|-]+\|[\s:|-]*$/.test(lines[i + 1])) {
        flushPara();
        const parseRow = (row) =>
          row
            .trim()
            .replace(/^\||\|$/g, "")
            .split("|")
            .map((c) => c.trim());
        const table = document.createElement("table");
        const thead = document.createElement("thead");
        const headTr = document.createElement("tr");
        for (const cell of parseRow(line)) {
          const th = document.createElement("th");
          th.appendChild(renderInline(cell, page));
          headTr.appendChild(th);
        }
        thead.appendChild(headTr);
        table.appendChild(thead);
        const tbody = document.createElement("tbody");
        i += 2;
        while (i < lines.length && /^\s*\|/.test(lines[i])) {
          const tr = document.createElement("tr");
          for (const cell of parseRow(lines[i])) {
            const td = document.createElement("td");
            td.appendChild(renderInline(cell, page));
            tr.appendChild(td);
          }
          tbody.appendChild(tr);
          i++;
        }
        table.appendChild(tbody);
        const wrap = document.createElement("div");
        wrap.className = "manual-table-wrap";
        wrap.appendChild(table);
        frag.appendChild(wrap);
        continue;
      }

      // blockquote
      if (/^\s*>/.test(line)) {
        flushPara();
        const buf = [];
        while (i < lines.length && /^\s*>/.test(lines[i])) {
          buf.push(lines[i].replace(/^\s*>\s?/, ""));
          i++;
        }
        const bq = document.createElement("blockquote");
        const p = document.createElement("p");
        p.appendChild(renderInline(buf.join(" "), page));
        bq.appendChild(p);
        frag.appendChild(bq);
        continue;
      }

      // horizontal rule
      if (/^\s*(-{3,}|\*{3,})\s*$/.test(line)) {
        flushPara();
        frag.appendChild(document.createElement("hr"));
        i++;
        continue;
      }

      // list (unordered / ordered, nested by 2-space indent)
      const lm = line.match(/^(\s*)([-*]|\d+\.)\s+(.*)$/);
      if (lm) {
        flushPara();
        const stack = []; // { list, indent }
        while (i < lines.length) {
          const m = lines[i].match(/^(\s*)([-*]|\d+\.)\s+(.*)$/);
          if (!m) break;
          const indent = m[1].length;
          const ordered = /\d/.test(m[2]);
          while (stack.length > 0 && indent < stack[stack.length - 1].indent) stack.pop();
          const top = stack[stack.length - 1];
          if (!top || indent > top.indent) {
            const list = document.createElement(ordered ? "ol" : "ul");
            if (top) {
              const items = top.list.children;
              (items[items.length - 1] || top.list).appendChild(list);
            } else {
              frag.appendChild(list);
            }
            stack.push({ list, indent });
          }
          const li = document.createElement("li");
          li.appendChild(renderInline(m[3], page));
          stack[stack.length - 1].list.appendChild(li);
          i++;
        }
        continue;
      }

      // paragraph / blank
      if (line.trim() === "") {
        flushPara();
      } else {
        para.push(line.trim());
      }
      i++;
    }
    flushPara();
    return frag;
  }

  // ── Viewer modal ──

  function makeHeaderButton(labelKey, fallback, onClick) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "ghost manual-nav-btn";
    btn.textContent = tr(labelKey, fallback);
    btn.dataset.i18n = labelKey;
    btn.addEventListener("click", onClick);
    return btn;
  }

  function ensureOverlay() {
    if (overlay) return;
    overlay = document.createElement("div");
    overlay.className = "modal-overlay manual-overlay hidden";

    const box = document.createElement("div");
    box.className = "manual-box";

    const header = document.createElement("div");
    header.className = "manual-header";
    backBtn = makeHeaderButton("manual_back", "Back", goBack);
    header.appendChild(backBtn);
    header.appendChild(makeHeaderButton("manual_toc", "Contents", () => openManual(HOME_PAGE)));
    const spacer = document.createElement("div");
    spacer.style.flex = "1";
    header.appendChild(spacer);
    header.appendChild(
      makeHeaderButton("manual_reload", "Reload", () => {
        if (current) {
          cache.delete(current);
          loadPage(current, null);
        }
      })
    );
    header.appendChild(
      makeHeaderButton("manual_open_github", "Open on GitHub", () => {
        if (current) openExternal(BLOB_BASE + current);
      })
    );
    const closeBtn = document.createElement("button");
    closeBtn.type = "button";
    closeBtn.className = "ghost manual-nav-btn manual-close-btn";
    closeBtn.textContent = "✕";
    closeBtn.addEventListener("click", closeManual);
    header.appendChild(closeBtn);
    box.appendChild(header);

    contentEl = document.createElement("div");
    contentEl.className = "manual-content";
    contentEl.addEventListener("click", (e) => {
      const a = e.target.closest("a");
      if (!a || !contentEl.contains(a)) return;
      e.preventDefault();
      if (a.dataset.external) {
        openExternal(a.dataset.external);
      } else if (a.dataset.mdPage) {
        openManual(a.dataset.mdPage, a.dataset.mdAnchor || null);
      }
    });
    box.appendChild(contentEl);

    overlay.appendChild(box);
    overlay.addEventListener("mousedown", (e) => {
      if (e.target === overlay) closeManual();
    });
    document.body.appendChild(overlay);
    if (typeof window.applyI18n === "function") window.applyI18n();
  }

  function isOpen() {
    return overlay !== null && !overlay.classList.contains("hidden");
  }

  function closeManual() {
    if (overlay) overlay.classList.add("hidden");
  }

  function goBack() {
    const prev = backStack.pop();
    if (prev) loadPage(prev.page, prev.anchor);
    updateBackBtn();
  }

  function updateBackBtn() {
    if (backBtn) backBtn.disabled = backStack.length === 0;
  }

  function scrollToAnchor(anchor) {
    if (!anchor) {
      contentEl.scrollTop = 0;
      return;
    }
    const el = contentEl.querySelector(`#${CSS.escape(anchor)}`);
    if (el) el.scrollIntoView();
    else contentEl.scrollTop = 0;
  }

  async function loadPage(page, anchor) {
    current = page;
    contentEl.textContent = tr("manual_loading", "Loading...");
    let md = cache.get(page);
    if (md === undefined) {
      try {
        const res = await fetch(RAW_BASE + page);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        md = await res.text();
        cache.set(page, md);
      } catch (_) {
        if (current !== page) return;
        contentEl.textContent = "";
        const p = document.createElement("p");
        p.className = "manual-error";
        p.textContent = tr("manual_load_error", "Failed to load the manual. Check your network connection.");
        const btn = document.createElement("button");
        btn.type = "button";
        btn.textContent = tr("manual_open_github", "Open on GitHub");
        btn.addEventListener("click", () => openExternal(BLOB_BASE + page));
        contentEl.appendChild(p);
        contentEl.appendChild(btn);
        return;
      }
    }
    if (current !== page) return; // superseded by another navigation
    contentEl.textContent = "";
    contentEl.appendChild(renderMarkdown(md, page));
    scrollToAnchor(anchor);
  }

  function openManual(page, anchor) {
    ensureOverlay();
    const target = page || HOME_PAGE;
    if (isOpen() && current && current !== target) {
      backStack.push({ page: current, anchor: null });
    }
    overlay.classList.remove("hidden");
    updateBackBtn();
    if (current === target && cache.has(target)) {
      scrollToAnchor(anchor);
      return;
    }
    loadPage(target, anchor);
  }

  // ── Global entry points ──

  document.addEventListener("click", (e) => {
    const el = e.target.closest("[data-manual]");
    if (!el) return;
    e.preventDefault();
    const [page, anchor] = el.getAttribute("data-manual").split("#");
    openManual(page, anchor || null);
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "F1") {
      e.preventDefault();
      openManual(current || HOME_PAGE);
    } else if (e.key === "Escape" && isOpen()) {
      e.stopPropagation();
      closeManual();
    }
  });

  // Reusable "?" button for dynamically built modals.
  function createHelpButton(spec) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "help-btn";
    btn.textContent = "?";
    btn.title = tr("help_open_manual", "Open manual");
    btn.setAttribute("data-manual", spec);
    btn.setAttribute("data-i18n-title", "help_open_manual");
    return btn;
  }

  window.openManual = openManual;
  window.createHelpButton = createHelpButton;
})();
