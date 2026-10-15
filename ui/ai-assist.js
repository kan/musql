// The AI settings and the AI assist chat, shared by the connection windows (query, Redis
// #98). The page holds the two modals (#ai-modal and #ai-assist-modal, with their parts)
// and defines `safeInvoke`. Needs i18n.js (`t`, `applyI18n`) and icons.js (`icon`).
// What to ask the AI and where its answer goes is the page's: see `openAiAssistModal`.

const aiModal = document.getElementById("ai-modal");
const aiProviderSelect = document.getElementById("ai-provider");
const aiModelInput = document.getElementById("ai-model");
const aiApiKeyInput = document.getElementById("ai-api-key");
const aiSaveBtn = document.getElementById("ai-save-btn");
const aiCancelBtn = document.getElementById("ai-cancel-btn");

const aiAssistModal = document.getElementById("ai-assist-modal");
const aiAssistMessages = document.getElementById("ai-assist-messages");
const aiAssistInput = document.getElementById("ai-assist-input");
const aiAssistSendBtn = document.getElementById("ai-assist-send-btn");
const aiAssistClearBtn = document.getElementById("ai-assist-clear-btn");
const aiAssistSettingsBtn = document.getElementById("ai-assist-settings-btn");
const aiAssistCloseBtn = document.getElementById("ai-assist-close-btn");

aiAssistSendBtn.innerHTML = icon('send');
aiAssistClearBtn.innerHTML = icon('eraser');
aiAssistClearBtn.title = t('ai_assist_clear');
aiAssistSettingsBtn.innerHTML = icon('settings');
aiAssistSettingsBtn.title = t('ai_settings');
aiAssistCloseBtn.innerHTML = icon('x');

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
// Asked when the settings first open (`showAiModal`): this file loads before the page's
// script, which defines `safeInvoke`.
let aiOpAvailable = null;

function aiOpRefKey(provider) {
  return `musql:ai:op-ref:${provider}`;
}

function getAiOpRef(provider) {
  return provider ? localStorage.getItem(aiOpRefKey(provider)) || "" : "";
}

function updateAiOpRowVisibility() {
  aiOpRow.classList.toggle("hidden", !aiOpAvailable || !aiProviderSelect.value);
}

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
  if (aiOpAvailable === null) {
    try { aiOpAvailable = !!(await safeInvoke("op_available")); }
    catch (_) { aiOpAvailable = false; }
  }
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

// What the page that opened the chat gave `openAiAssistModal`; null while it is closed.
let aiAssistSession = null;
const AI_CHAT_MAX = 50;

// `key` is a session's chatKey; null keeps no history.
function loadAiChat(key) {
  if (!key) return [];
  try { return JSON.parse(localStorage.getItem(key)) || []; }
  catch (_) { return []; }
}

function saveAiChat(key, messages) {
  if (!key) return;
  if (messages.length > AI_CHAT_MAX) {
    messages = messages.slice(messages.length - AI_CHAT_MAX);
  }
  localStorage.setItem(key, JSON.stringify(messages));
}

// The localStorage key of a connection's chat history: per profile and database (the DB
// number for Redis). null keeps none (not connected).
function aiChatKey(profileId, database) {
  if (!profileId || database === null || database === undefined || database === "") return null;
  return "musql:ai-chat:" + profileId + ":" + database;
}

// Opens the chat. `session` is the page's part:
// - chatKey: the localStorage key of the chat history (`aiChatKey`)
// - placeholder, emptyText: the input's placeholder, and what an empty chat says
// - request(prompt, conversationContext, ai): the answer, from Rust; `ai` holds the
//   provider, model and 1Password reference to send along
// - insertLabel, insert(text): the button that puts an answer to use, and what it does
// - perLine: a multi-line answer gets a button per line (a page that takes one line at
//   a time, such as the Redis CLI)
function openAiAssistModal(session) {
  aiAssistSession = session;
  aiAssistInput.placeholder = session.placeholder;
  renderAiMessages();
  aiAssistModal.classList.remove("hidden");
  aiAssistInput.focus();
}

function closeAiAssistModal() {
  aiAssistModal.classList.add("hidden");
  aiAssistSession = null;
}

function aiMessageButton(iconName, label, onClick) {
  const btn = document.createElement("button");
  btn.className = "ghost";
  btn.innerHTML = icon(iconName, 14) + " " + label;
  btn.addEventListener("click", onClick);
  return btn;
}

function aiInsertButton(text) {
  return aiMessageButton("terminal", aiAssistSession.insertLabel, () => {
    const session = aiAssistSession;
    closeAiAssistModal();
    session.insert(text);
  });
}

function renderAiMessages() {
  aiAssistMessages.innerHTML = "";
  const messages = loadAiChat(aiAssistSession.chatKey);
  if (messages.length === 0) {
    const empty = document.createElement("div");
    empty.className = "ai-assist-empty";
    empty.textContent = aiAssistSession.emptyText;
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
      const lines = aiAssistSession.perLine ? msg.content.split("\n").filter((line) => line.trim()) : [];
      const perLine = lines.length > 1;
      if (perLine) {
        lines.forEach((line) => {
          const row = document.createElement("div");
          row.className = "ai-msg-line";
          const pre = document.createElement("pre");
          pre.textContent = line;
          row.appendChild(pre);
          row.appendChild(aiInsertButton(line));
          el.appendChild(row);
        });
      } else {
        const pre = document.createElement("pre");
        pre.textContent = msg.content;
        el.appendChild(pre);
      }
      const actions = document.createElement("div");
      actions.className = "ai-msg-actions";
      actions.appendChild(aiMessageButton("copy", t('copy'), () => {
        navigator.clipboard.writeText(msg.content).catch(() => {});
      }));
      if (!perLine) {
        // A page that takes one line gets the line alone, without blank lines around it.
        actions.appendChild(aiInsertButton(lines.length === 1 ? lines[0].trim() : msg.content));
      }
      el.appendChild(actions);
    }
    aiAssistMessages.appendChild(el);
  });
  aiAssistMessages.scrollTop = aiAssistMessages.scrollHeight;
}

async function sendAiAssistMessage() {
  const prompt = aiAssistInput.value.trim();
  // One request at a time (Enter as well as the button): a second one would be asked
  // without the first one's answer, and the answers would land in either order.
  if (!prompt || !aiAssistSession || aiAssistSendBtn.disabled) return;
  const session = aiAssistSession;
  aiAssistSendBtn.disabled = true;

  const configured = await isAiConfigured();
  if (!configured) {
    aiAssistSendBtn.disabled = false;
    alert(t('ai_not_configured'));
    showAiModal();
    return;
  }
  // Closed (or opened for another connection) while asking: the prompt was not sent.
  if (aiAssistSession !== session) {
    aiAssistSendBtn.disabled = false;
    return;
  }

  const provider = getAiProvider();
  const ai = { provider, model: getAiModel(), opRef: getAiOpRef(provider) || null };

  // Add user message
  const messages = loadAiChat(session.chatKey);
  messages.push({ role: "user", content: prompt, ts: Date.now() });
  saveAiChat(session.chatKey, messages);
  aiAssistInput.value = "";
  renderAiMessages();

  // Show progress
  const progressEl = document.createElement("div");
  progressEl.className = "ai-assist-progress";
  progressEl.innerHTML = '<span class="spinner"></span>' + t('ai_requesting');
  aiAssistMessages.appendChild(progressEl);
  aiAssistMessages.scrollTop = aiAssistMessages.scrollHeight;

  // Build conversation context (last few messages)
  const conversationContext = messages.slice(-10).map((m) => {
    return (m.role === "user" ? "User" : "Assistant") + ": " + m.content;
  }).join("\n");

  let reply;
  try {
    reply = { role: "assistant", content: await session.request(prompt, conversationContext, ai) };
  } catch (e) {
    reply = { role: "error", content: String(e) };
  }
  aiAssistSendBtn.disabled = false;
  progressEl.remove();
  // The answer goes to the history it was asked from, even when the chat was closed (or
  // opened for another connection) while waiting.
  const updated = loadAiChat(session.chatKey);
  updated.push({ ...reply, ts: Date.now() });
  saveAiChat(session.chatKey, updated);
  if (aiAssistSession === session) renderAiMessages();
}

aiAssistSendBtn.addEventListener("click", sendAiAssistMessage);

aiAssistInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && !e.shiftKey) {
    e.preventDefault();
    sendAiAssistMessage();
  }
});

aiAssistCloseBtn.addEventListener("click", closeAiAssistModal);

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !aiAssistModal.classList.contains("hidden")) {
    closeAiAssistModal();
  }
});

aiAssistModal.addEventListener("click", (e) => {
  if (e.target === aiAssistModal) closeAiAssistModal();
});

aiAssistClearBtn.addEventListener("click", () => {
  const key = aiAssistSession && aiAssistSession.chatKey;
  if (key) localStorage.removeItem(key);
  renderAiMessages();
});

aiAssistSettingsBtn.addEventListener("click", () => {
  showAiModal();
});

window.showAiModal = showAiModal;
window.hideAiModal = hideAiModal;
window.aiChatKey = aiChatKey;
window.openAiAssistModal = openAiAssistModal;
window.closeAiAssistModal = closeAiAssistModal;
