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

// AI modal DOM refs
const aiModal = document.getElementById("ai-modal");
const aiProviderSelect = document.getElementById("ai-provider");
const aiModelInput = document.getElementById("ai-model");
const aiApiKeyInput = document.getElementById("ai-api-key");
const aiSaveBtn = document.getElementById("ai-save-btn");
const aiCancelBtn = document.getElementById("ai-cancel-btn");

// AI Assist modal DOM refs
const aiAssistModal = document.getElementById("ai-assist-modal");
const aiAssistMessages = document.getElementById("ai-assist-messages");
const aiAssistInput = document.getElementById("ai-assist-input");
const aiAssistSendBtn = document.getElementById("ai-assist-send-btn");
const aiAssistClearBtn = document.getElementById("ai-assist-clear-btn");
const aiAssistSettingsBtn = document.getElementById("ai-assist-settings-btn");
const aiAssistCloseBtn = document.getElementById("ai-assist-close-btn");

// Apply icons to static elements
menuBtn.innerHTML = icon('menu');
dbSwitchBtn.innerHTML = icon('arrow-left-right');
tabAddBtn.innerHTML = icon('plus');
aiAssistSendBtn.innerHTML = icon('send');
aiAssistClearBtn.innerHTML = icon('eraser');
aiAssistClearBtn.title = t('ai_assist_clear');
aiAssistSettingsBtn.innerHTML = icon('settings');
aiAssistSettingsBtn.title = t('ai_settings');
aiAssistCloseBtn.innerHTML = icon('x');

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

// ── AI settings modal ──

const AI_MODELS = {
  claude: [
    { value: "claude-sonnet-5", label: "Claude Sonnet 5" },
    { value: "claude-haiku-4-5", label: "Claude Haiku 4.5" },
    { value: "claude-opus-4-8", label: "Claude Opus 4.8" },
  ],
  openai: [
    { value: "gpt-5.4", label: "GPT-5.4" },
    { value: "gpt-5.4-mini", label: "GPT-5.4 Mini" },
    { value: "gpt-5.4-nano", label: "GPT-5.4 Nano" },
    { value: "gpt-5.5", label: "GPT-5.5" },
  ],
  gemini: [
    { value: "gemini-3.5-flash", label: "Gemini 3.5 Flash" },
    { value: "gemini-3.1-flash-lite", label: "Gemini 3.1 Flash-Lite" },
    { value: "gemini-3.1-pro-preview", label: "Gemini 3.1 Pro (Preview)" },
  ],
};

function aiDefaultModel(provider) {
  const models = AI_MODELS[provider];
  return models && models.length > 0 ? models[0].value : "";
}

function getAiProvider() {
  return localStorage.getItem("musql:ai:provider") || "";
}

function getAiModel() {
  return localStorage.getItem("musql:ai:model") || "";
}

function populateAiModelSelect(provider, selectedModel) {
  const select = aiModelInput;
  select.innerHTML = "";
  const models = AI_MODELS[provider] || [];
  if (models.length === 0) {
    const opt = document.createElement("option");
    opt.value = "";
    opt.textContent = "--";
    select.appendChild(opt);
    return;
  }
  models.forEach((m) => {
    const opt = document.createElement("option");
    opt.value = m.value;
    opt.textContent = m.label;
    select.appendChild(opt);
  });
  if (selectedModel && models.some((m) => m.value === selectedModel)) {
    select.value = selectedModel;
  } else {
    select.value = models[0].value;
  }
}

async function isAiConfigured() {
  const provider = getAiProvider();
  if (!provider) return false;
  try {
    if (await safeInvoke("has_ai_api_key", { provider })) return true;
  } catch (_) {
    // Fall through: a 1Password reference still counts as configured.
  }
  // A 1Password reference counts as configured — the key is fetched on first use.
  return !!getAiOpRef(provider);
}

// ── 1Password (AI API key) ──
// The reference lives in localStorage rather than the profile store: AI settings are
// per-machine and are not part of what syncs. It seeds the keyring on first use, exactly
// like the connection credentials.
const aiOpRefInput = document.getElementById("ai-op-ref");
const aiOpRow = document.getElementById("ai-op-row");
let aiOpAvailable = false;

function aiOpRefKey(provider) {
  return `musql:ai:op-ref:${provider}`;
}

function getAiOpRef(provider) {
  return provider ? localStorage.getItem(aiOpRefKey(provider)) || "" : "";
}

function updateAiOpRowVisibility() {
  aiOpRow.classList.toggle("hidden", !aiOpAvailable || !aiProviderSelect.value);
}

safeInvoke("op_available")
  .then((available) => {
    aiOpAvailable = !!available;
    updateAiOpRowVisibility();
  })
  .catch(() => {});

const aiOpStatus = document.getElementById("ai-op-status");

function setAiOpStatus(message, ok) {
  aiOpStatus.textContent = message;
  aiOpStatus.style.color = ok ? "var(--success)" : "var(--danger)";
}

const aiOpBrowseBtn = document.getElementById("ai-op-browse");
const aiOpFetchBtn = document.getElementById("ai-op-fetch");

async function fetchAiKeyFromOnePassword() {
  const reference = aiOpRefInput.value.trim();
  if (!reference) return;
  // Both buttons would start another CLI call on top of the one in flight.
  aiOpBrowseBtn.disabled = true;
  aiOpFetchBtn.disabled = true;
  setAiOpStatus(t('op_fetching'), true);
  try {
    aiApiKeyInput.value = await safeInvoke("op_read_secret", { reference });
    aiApiKeyInput.placeholder = "";
    setAiOpStatus(t('op_fetched'), true);
  } catch (e) {
    setAiOpStatus(String(e), false);
  } finally {
    aiOpBrowseBtn.disabled = false;
    aiOpFetchBtn.disabled = false;
  }
}

aiOpBrowseBtn.addEventListener("click", async () => {
  if (typeof window.openOpPicker !== "function") return;
  const reference = await window.openOpPicker();
  if (!reference) return;
  aiOpRefInput.value = reference;
  // Picking a field settles what the key is, so fetch it without a second click.
  await fetchAiKeyFromOnePassword();
});

aiOpFetchBtn.addEventListener("click", fetchAiKeyFromOnePassword);

async function showAiModal() {
  const provider = getAiProvider();
  const model = getAiModel();
  aiProviderSelect.value = provider;
  populateAiModelSelect(provider, model);
  aiApiKeyInput.value = "";
  aiOpRefInput.value = getAiOpRef(provider);
  setAiOpStatus("", true);
  updateAiOpRowVisibility();
  if (provider) {
    try {
      const hasKey = await safeInvoke("has_ai_api_key", { provider });
      if (hasKey) aiApiKeyInput.placeholder = "(saved)";
      else aiApiKeyInput.placeholder = "";
    } catch (_) {
      aiApiKeyInput.placeholder = "";
    }
  } else {
    aiApiKeyInput.placeholder = "";
  }
  aiModal.classList.remove("hidden");
  applyI18n();
}

function hideAiModal() {
  aiModal.classList.add("hidden");
}

aiProviderSelect.addEventListener("change", () => {
  const p = aiProviderSelect.value;
  populateAiModelSelect(p, "");
  aiApiKeyInput.value = "";
  aiApiKeyInput.placeholder = "";
  aiOpRefInput.value = getAiOpRef(p);
  setAiOpStatus("", true);
  updateAiOpRowVisibility();
  if (p) {
    safeInvoke("has_ai_api_key", { provider: p }).then((hasKey) => {
      if (hasKey) aiApiKeyInput.placeholder = "(saved)";
    }).catch(() => {});
  }
});

aiSaveBtn.addEventListener("click", async () => {
  const provider = aiProviderSelect.value;
  const model = aiModelInput.value;
  if (provider) {
    localStorage.setItem("musql:ai:provider", provider);
    localStorage.setItem("musql:ai:model", model || aiDefaultModel(provider));
    const opRef = aiOpRefInput.value.trim();
    if (opRef) localStorage.setItem(aiOpRefKey(provider), opRef);
    else localStorage.removeItem(aiOpRefKey(provider));
    const key = aiApiKeyInput.value.trim();
    if (key) {
      try { await safeInvoke("save_ai_api_key", { provider, apiKey: key }); }
      catch (e) { console.warn("Failed to save AI API key:", e); }
    }
  } else {
    localStorage.removeItem("musql:ai:provider");
    localStorage.removeItem("musql:ai:model");
  }
  hideAiModal();
});

aiCancelBtn.addEventListener("click", hideAiModal);

aiModal.addEventListener("click", (e) => {
  if (e.target === aiModal) hideAiModal();
});

// ── AI Assist modal ──

let aiAssistEditor = null; // reference to the editor that opened the modal
const AI_CHAT_MAX = 50;

function getAiChatKey() {
  if (!currentProfileId || !currentDb) return null;
  return "musql:ai-chat:" + currentProfileId + ":" + currentDb;
}

function loadAiChat() {
  const key = getAiChatKey();
  if (!key) return [];
  try { return JSON.parse(localStorage.getItem(key)) || []; }
  catch (_) { return []; }
}

function saveAiChat(messages) {
  const key = getAiChatKey();
  if (!key) return;
  if (messages.length > AI_CHAT_MAX) {
    messages = messages.slice(messages.length - AI_CHAT_MAX);
  }
  localStorage.setItem(key, JSON.stringify(messages));
}

function openAiAssistModal(editor) {
  aiAssistEditor = editor;
  aiAssistInput.placeholder = t('ai_assist_placeholder');
  renderAiMessages();
  aiAssistModal.classList.remove("hidden");
  aiAssistInput.focus();
}

function closeAiAssistModal() {
  aiAssistModal.classList.add("hidden");
  aiAssistEditor = null;
}

function renderAiMessages() {
  aiAssistMessages.innerHTML = "";
  const messages = loadAiChat();
  if (messages.length === 0) {
    const empty = document.createElement("div");
    empty.className = "ai-assist-empty";
    empty.textContent = t('ai_assist_empty');
    aiAssistMessages.appendChild(empty);
    return;
  }
  messages.forEach((msg) => {
    const el = document.createElement("div");
    if (msg.role === "user") {
      el.className = "ai-msg user";
      el.textContent = msg.content;
    } else if (msg.role === "error") {
      el.className = "ai-msg error";
      el.textContent = msg.content;
    } else {
      el.className = "ai-msg assistant";
      const pre = document.createElement("pre");
      pre.textContent = msg.content;
      el.appendChild(pre);
      const actions = document.createElement("div");
      actions.className = "ai-msg-actions";
      const copyBtn = document.createElement("button");
      copyBtn.className = "ghost";
      copyBtn.innerHTML = icon('copy', 14) + " " + t('ai_assist_copy');
      copyBtn.addEventListener("click", () => {
        navigator.clipboard.writeText(msg.content).catch(() => {});
      });
      actions.appendChild(copyBtn);
      const insertBtn = document.createElement("button");
      insertBtn.className = "ghost";
      insertBtn.innerHTML = icon('terminal', 14) + " " + t('ai_assist_insert');
      insertBtn.addEventListener("click", () => {
        if (aiAssistEditor) {
          const cursor = aiAssistEditor.getCursor();
          aiAssistEditor.replaceRange(msg.content + "\n", cursor);
          closeAiAssistModal();
          aiAssistEditor.focus();
        }
      });
      actions.appendChild(insertBtn);
      el.appendChild(actions);
    }
    aiAssistMessages.appendChild(el);
  });
  aiAssistMessages.scrollTop = aiAssistMessages.scrollHeight;
}

async function sendAiAssistMessage() {
  const prompt = aiAssistInput.value.trim();
  if (!prompt) return;

  const configured = await isAiConfigured();
  if (!configured) {
    alert(t('ai_not_configured'));
    showAiModal();
    return;
  }

  const provider = getAiProvider();
  const model = getAiModel();

  // Add user message
  const messages = loadAiChat();
  messages.push({ role: "user", content: prompt, ts: Date.now() });
  saveAiChat(messages);
  aiAssistInput.value = "";
  renderAiMessages();

  // Show progress
  const progressEl = document.createElement("div");
  progressEl.className = "ai-assist-progress";
  progressEl.innerHTML = '<span class="spinner"></span>' + t('ai_requesting');
  aiAssistMessages.appendChild(progressEl);
  aiAssistMessages.scrollTop = aiAssistMessages.scrollHeight;
  aiAssistSendBtn.disabled = true;

  // Build conversation context (last few messages)
  const conversationContext = messages.slice(-10).map((m) => {
    return (m.role === "user" ? "User" : "Assistant") + ": " + m.content;
  }).join("\n");

  const editorContent = aiAssistEditor ? aiAssistEditor.getValue() : "";

  try {
    const result = await safeInvoke("ai_assist", {
      prompt,
      editorContent,
      conversationContext,
      provider,
      model,
      database: currentDb,
      opRef: getAiOpRef(provider) || null,
    });
    progressEl.remove();
    const updated = loadAiChat();
    updated.push({ role: "assistant", content: result, ts: Date.now() });
    saveAiChat(updated);
    renderAiMessages();
  } catch (e) {
    progressEl.remove();
    const updated = loadAiChat();
    updated.push({ role: "error", content: String(e), ts: Date.now() });
    saveAiChat(updated);
    renderAiMessages();
  } finally {
    aiAssistSendBtn.disabled = false;
  }
}

aiAssistSendBtn.addEventListener("click", sendAiAssistMessage);

aiAssistInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendAiAssistMessage();
  }
});

aiAssistCloseBtn.addEventListener("click", closeAiAssistModal);

aiAssistModal.addEventListener("click", (e) => {
  if (e.target === aiAssistModal) closeAiAssistModal();
});

aiAssistClearBtn.addEventListener("click", () => {
  const key = getAiChatKey();
  if (key) localStorage.removeItem(key);
  renderAiMessages();
});

aiAssistSettingsBtn.addEventListener("click", () => {
  showAiModal();
});

// ── Helpers ──

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

// ── Credential prompt modal ──
const pwPromptModal = document.getElementById("pw-prompt-modal");
const pwPromptFields = document.getElementById("pw-prompt-fields");
const pwPromptError = document.getElementById("pw-prompt-error");
const pwPromptOk = document.getElementById("pw-prompt-ok");
const pwPromptCancel = document.getElementById("pw-prompt-cancel");

function promptForCredentials(request) {
  return new Promise((resolve) => {
    const fields = [];
    if (request.mysql.save_password === false) {
      fields.push({ key: "mysql_password", label: t("password"), type: "password" });
    }
    if (request.ssh && request.ssh.enabled) {
      if (request.ssh.auth_method === "password" && request.ssh.save_ssh_password === false) {
        fields.push({ key: "ssh_password", label: t("ssh_password_label"), type: "password" });
      }
      if (request.ssh.auth_method === "key" && request.ssh.save_ssh_passphrase === false) {
        fields.push({ key: "ssh_passphrase", label: t("ssh_passphrase"), type: "password" });
      }
    }
    if (fields.length === 0) {
      resolve(true);
      return;
    }

    pwPromptFields.innerHTML = "";
    pwPromptError.textContent = "";
    const inputs = {};
    fields.forEach((f) => {
      const lbl = document.createElement("label");
      lbl.style.marginBottom = "10px";
      const span = document.createElement("span");
      span.textContent = f.label;
      const inp = document.createElement("input");
      inp.type = f.type;
      inp.style.width = "100%";
      lbl.appendChild(span);
      lbl.appendChild(inp);
      pwPromptFields.appendChild(lbl);
      inputs[f.key] = inp;
    });

    pwPromptModal.classList.remove("hidden");
    const firstInput = Object.values(inputs)[0];
    if (firstInput) firstInput.focus();

    function cleanup() {
      pwPromptModal.classList.add("hidden");
      pwPromptOk.removeEventListener("click", onOk);
      pwPromptCancel.removeEventListener("click", onCancel);
      document.removeEventListener("keydown", onKey);
    }
    function onOk() {
      if (inputs.mysql_password) {
        request.mysql.password = inputs.mysql_password.value;
      }
      if (inputs.ssh_password && request.ssh) {
        request.ssh.ssh_password = inputs.ssh_password.value;
      }
      if (inputs.ssh_passphrase && request.ssh) {
        request.ssh.passphrase = inputs.ssh_passphrase.value;
      }
      cleanup();
      resolve(true);
    }
    function onCancel() {
      cleanup();
      resolve(false);
    }
    function onKey(e) {
      if (e.key === "Escape") onCancel();
      if (e.key === "Enter") onOk();
    }
    pwPromptOk.addEventListener("click", onOk);
    pwPromptCancel.addEventListener("click", onCancel);
    document.addEventListener("keydown", onKey);
  });
}

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
// while the window is unfocused. Web Notification API first (supports click-to-focus),
// tauri-plugin-notification as fallback (no click handling), matching pike.
const NOTIFY_THRESHOLD_MS = 5000;

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
// Click-to-focus is intentionally not wired: WebView2 does not deliver Web Notification
// `onclick`, and notify_rust has no click callback on Windows desktop.
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

async function maybeNotifyQueryDone(elapsedMs, body) {
  if (!isNotifyEnabled() || elapsedMs < NOTIFY_THRESHOLD_MS) return;
  if (document.hasFocus()) return; // user is already looking at the window
  const notify = await resolveNotifier();
  if (!notify) return;
  // Re-check focus: resolving may have shown a permission prompt that the user
  // dismissed by refocusing the window.
  if (document.hasFocus()) return;
  try { notify(sidebarDbName.textContent || "muSQL", body); }
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
  const win = window.__TAURI__ && window.__TAURI__.webviewWindow;
  if (win) {
    win.getCurrentWebviewWindow().setTitle(title).catch(() => {});
  }
}

// ── Export utilities ──

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

function quoteId(name) {
  return '`' + name.replace(/`/g, '``') + '`';
}

function sqlEscapeValue(val) {
  if (val === null || val === undefined) return "NULL";
  if (typeof val === "number") return String(val);
  return "'" + String(val).replace(/\\/g, "\\\\").replace(/'/g, "\\'") + "'";
}

async function generateInsertSql(tableName, newline) {
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

async function generateMarkdownSchema(tableName, newline) {
  const escaped = tableName.replace(/'/g, "''");
  const [createRes, descRes, indexRes, commentRes, colCommentRes, fkParentRes, fkChildRes] = await Promise.all([
    runQuery("SHOW CREATE TABLE " + quoteId(tableName)).catch(() => null),
    runQuery("DESCRIBE " + quoteId(tableName)),
    runQuery("SHOW INDEX FROM " + quoteId(tableName)),
    runQuery("SELECT TABLE_COMMENT FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '" + escaped + "'"),
    runQuery("SELECT COLUMN_NAME, COLUMN_COMMENT FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '" + escaped + "' ORDER BY ORDINAL_POSITION"),
    runQuery("SELECT CONSTRAINT_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '" + escaped + "' AND REFERENCED_TABLE_NAME IS NOT NULL"),
    runQuery("SELECT TABLE_NAME, COLUMN_NAME, CONSTRAINT_NAME FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA = DATABASE() AND REFERENCED_TABLE_NAME = '" + escaped + "'"),
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

  const lines = [];

  // Title
  lines.push("# " + tableName);
  lines.push("");

  // Description
  const tableComment = (commentRes && commentRes.rows && commentRes.rows.length > 0) ? (commentRes.rows[0][0] || "") : "";
  if (tableComment) {
    lines.push("## Description");
    lines.push("");
    lines.push(tableComment);
    lines.push("");
  }

  // Table Definition
  if (createRes && createRes.rows && createRes.rows.length > 0) {
    const createSql = createRes.rows[0][1] || "";
    if (createSql) {
      lines.push("<details>");
      lines.push("<summary><strong>Table Definition</strong></summary>");
      lines.push("");
      lines.push("```sql");
      createSql.split(/\r?\n/).forEach((l) => lines.push(l));
      lines.push("```");
      lines.push("");
      lines.push("</details>");
      lines.push("");
    }
  }

  // Columns
  if (descRes && descRes.rows && descRes.rows.length > 0) {
    const ci = {};
    for (let i = 0; i < descRes.columns.length; i++) ci[descRes.columns[i]] = i;

    lines.push("## Columns");
    lines.push("");
    lines.push("| Name | Type | Default | Nullable | Extra Definition | Children | Parents | Comment |");
    lines.push("| ---- | ---- | ------- | -------- | ---------------- | -------- | ------- | ------- |");

    for (const row of descRes.rows) {
      const field = row[ci["Field"]] || "";
      const type = row[ci["Type"]] || "";
      const nullable = (row[ci["Null"]] === "YES") ? "true" : "false";
      const defVal = row[ci["Default"]];
      const defaultStr = (defVal === null || defVal === undefined) ? "" : String(defVal);
      const extra = row[ci["Extra"]] || "";
      const comment = (colComments[field] || "").replace(/\|/g, "\\|");

      const children = (fkChildren[field] || []).map(c => "[" + c.table + "](" + c.table + ".md)").join(" ");
      const parents = (fkParents[field] || []).map(p => "[" + p.table + "](" + p.table + ".md)").join(" ");

      lines.push("| " + field + " | " + type + " | " + defaultStr + " | " + nullable + " | " + extra + " | " + children + " | " + parents + " | " + comment + " |");
    }
    lines.push("");
  }

  // Constraints
  const constraints = [];
  for (const [keyName, idx] of Object.entries(indexMap)) {
    const cols = idx.columns.map(c => c.col).join(", ");
    if (keyName === "PRIMARY") {
      constraints.push({ name: "PRIMARY", type: "PRIMARY KEY", definition: "PRIMARY KEY (" + cols + ")" });
    } else if (idx.nonUnique === 0) {
      constraints.push({ name: keyName, type: "UNIQUE", definition: "UNIQUE KEY " + keyName + " (" + cols + ")" });
    }
  }
  for (const [name, fk] of Object.entries(fkConstraints)) {
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
  const indexEntries = Object.entries(indexMap);
  if (indexEntries.length > 0) {
    lines.push("## Indexes");
    lines.push("");
    lines.push("| Name | Definition |");
    lines.push("| ---- | ---------- |");
    for (const [keyName, idx] of indexEntries) {
      const cols = idx.columns.map(c => c.col).join(", ");
      let prefix;
      if (idx.nonUnique === 0 && keyName === "PRIMARY") prefix = "PRIMARY KEY";
      else if (idx.nonUnique === 0) prefix = "UNIQUE KEY";
      else prefix = "KEY";
      lines.push("| " + keyName + " | " + prefix + " (" + cols + ") USING " + idx.indexType + " |");
    }
    lines.push("");
  }

  // Footer
  lines.push("---");
  lines.push("");
  lines.push("> Generated by [muSQL](https://github.com/nicoyou/musql) ([tbls](https://github.com/k1LoW/tbls) compatible)");

  return lines.join(newline || "\n");
}

// ── Export options: encoding + line ending + clipboard (#40) ──

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

async function saveFile(content, defaultName, filterName, extensions, encoding) {
  return safeInvoke("export_file", { content, defaultName, filterName, extensions, encoding: encoding || "utf-8" });
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
    items.push({ separator: true });
    items.push({ label: t('markdown_schema'), icon: "download", action: () => doExportMarkdownSchema(tableName) });
  }
  showContextMenu(e, items);
}

function doExportCurrent(columns, rows, tableName, sep, ext) {
  openExportDialog({
    defaultName: (tableName || "export") + "." + ext,
    ext,
    filterName: ext.toUpperCase(),
    generate: async (nl) => generateCsv(columns, rows, sep, nl),
  });
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
    if (ci === ss.colIndex && ss.dir === "asc") { indicator.textContent = " \u25B2"; th.classList.add("sort-active"); }
    else if (ci === ss.colIndex && ss.dir === "desc") { indicator.textContent = " \u25BC"; th.classList.add("sort-active"); }
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
    if (onSortCallback || ss.colIndex < 0 || ss.dir === null) return rows;
    const sorted = rows.slice();
    const ci = ss.colIndex;
    const dir = ss.dir === "asc" ? 1 : -1;
    sorted.sort((a, b) => {
      const va = a[ci];
      const vb = b[ci];
      if ((va === null || va === undefined) && (vb === null || vb === undefined)) return 0;
      if (va === null || va === undefined) return 1;
      if (vb === null || vb === undefined) return -1;
      if (va === "" && vb === "") return 0;
      if (va === "") return 1;
      if (vb === "") return -1;
      if (typeof va === "number" && typeof vb === "number") return (va - vb) * dir;
      const na = Number(va);
      const nb = Number(vb);
      if (!Number.isNaN(na) && !Number.isNaN(nb) && va !== "" && vb !== "") return (na - nb) * dir;
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

// Client-side row sort matching renderTable's comparator (used for paged SQL results)
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

async function showExplorer() {
  explorerEl.classList.remove("hidden");
  sidebarDbName.textContent = currentDb;
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
    const res = await runQuery("SHOW TABLES");
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

    // Column metadata from INFORMATION_SCHEMA
    let allColumns = [];
    let blobCols = [];
    let textCols = [];
    let pkCols = [];
    let truncateMode = true;
    let hasTruncatable = false;
    let columnsDetected = false;

    async function detectColumns() {
      if (columnsDetected) return;
      const sql = "SELECT COLUMN_NAME, DATA_TYPE, COLUMN_KEY " +
        "FROM INFORMATION_SCHEMA.COLUMNS " +
        "WHERE TABLE_SCHEMA = '" + currentDb.replace(/'/g, "''") + "' " +
        "AND TABLE_NAME = '" + tableName.replace(/'/g, "''") + "' " +
        "ORDER BY ORDINAL_POSITION";
      const res = await runQuery(sql);
      allColumns = res.rows.map((r) => ({ name: r[0], dataType: (r[1] || "").toLowerCase(), columnKey: r[2] || "" }));
      const blobTypes = ["blob", "tinyblob", "mediumblob", "longblob", "binary", "varbinary"];
      const textTypes = ["text", "tinytext", "mediumtext", "longtext"];
      blobCols = allColumns.filter((c) => blobTypes.includes(c.dataType)).map((c) => c.name);
      textCols = allColumns.filter((c) => textTypes.includes(c.dataType)).map((c) => c.name);
      pkCols = allColumns.filter((c) => c.columnKey === "PRI").map((c) => c.name);
      hasTruncatable = blobCols.length > 0 || textCols.length > 0;
      columnsDetected = true;
    }

    function buildSelectExpr() {
      if (!truncateMode || !hasTruncatable) return "*";
      return allColumns.map((c) => {
        const q = quoteId(c.name);
        if (blobCols.includes(c.name)) return "'(BLOB)' AS " + q;
        if (textCols.includes(c.name)) return "CASE WHEN CHAR_LENGTH(" + q + ") > 200 THEN CONCAT(LEFT(" + q + ", 200), '\u2026') ELSE " + q + " END AS " + q;
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
          return quoteId(pk) + " = '" + String(val).replace(/'/g, "''") + "'";
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
          "SELECT " + selectExpr + " FROM " + quoteId(tableName) + orderBy + " LIMIT " + offset + ", " + pageSize
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
        const [descRes, indexRes] = await Promise.all([
          runQuery("DESCRIBE " + quoteId(tableName)),
          runQuery("SHOW INDEX FROM " + quoteId(tableName)),
        ]);
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
      mode: "text/x-mysql",
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

    const aiAssistBtn = document.createElement("button");
    aiAssistBtn.className = "ghost";
    aiAssistBtn.innerHTML = icon('sparkles') + " " + t('ai_assist');
    aiAssistBtn.addEventListener("click", () => openAiAssistModal(editor));
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
          maybeNotifyQueryDone(performance.now() - execStart, notifyBody);
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

document.addEventListener("click", hideContextMenu);
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    hideContextMenu();
    if (!aiAssistModal.classList.contains("hidden")) {
      closeAiAssistModal();
    }
  }
});

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

  eventApi.listen("query:docker-open", async (event) => {
    const info = event.payload;
    if (!info) return;

    resetExplorer();
    dockerTunnelContainerId = info.tunnel_container_id || null;
    currentProfileId = "docker:" + info.name;
    currentProfileName = info.name;

    // Build a synthetic requestCache matching ConnectionRequest shape
    requestCache = {
      mysql: {
        host: info.host,
        port: info.port,
        database: null,
        username: info.user,
        password: info.password,
        ssl_mode: info.ssl_mode || "DISABLED",
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
    resetExplorer();
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
