const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;
const eventApi = window.__TAURI__ && window.__TAURI__.event ? window.__TAURI__.event : null;
const profileNameInput = document.getElementById("profile-name");
const resultEl = document.getElementById("result");
const testBtn = document.getElementById("test-btn");
const saveBtn = document.getElementById("profile-save");
const cancelBtn = document.getElementById("profile-cancel");
const deleteBtn = document.getElementById("profile-delete");
const connectBtn = document.getElementById("connect-btn");
const sshConfigHostSelect = document.getElementById("ssh-config-host");
const sshEnabledCheck = document.getElementById("ssh-enabled");
const sshFieldsDiv = document.getElementById("ssh-fields");
const sshManualFieldIds = ["ssh-host", "ssh-port", "ssh-user", "ssh-key"];
const sslModeSelect = document.getElementById("mysql-ssl-mode");
const caCertRow = document.getElementById("ca-cert-row");
const caCertInput = document.getElementById("mysql-ca-cert");
const caCertBrowseBtn = document.getElementById("mysql-ca-cert-browse");
const sshKeyBrowseBtn = document.getElementById("ssh-key-browse");

const menuBtn = document.getElementById("menu-btn");
menuBtn.innerHTML = icon('menu');

// Apply icons to heading and buttons
function applySettingsLabels() {
  document.getElementById("settings-heading").innerHTML = icon('settings', 24) + ' ' + t('settings_heading');
  testBtn.innerHTML = icon('zap') + t('test_connection');
  connectBtn.innerHTML = icon('play') + t('connect');
  saveBtn.innerHTML = icon('save') + t('save');
  cancelBtn.innerHTML = icon('x') + t('cancel');
  deleteBtn.innerHTML = icon('trash-2') + t('delete');
  caCertBrowseBtn.innerHTML = icon('folder-open') + t('browse');
  sshKeyBrowseBtn.innerHTML = icon('folder-open') + t('browse');
}
applySettingsLabels();

let selectedProfileId = "";
let selectedGroupId = null;
let selectedOrder = 0;

// ── Color / Tag state ──
const COLOR_PALETTE = [
  { name: "red", value: "#d24a4a" },
  { name: "orange", value: "#e67e22" },
  { name: "yellow", value: "#f1c40f" },
  { name: "green", value: "#27ae60" },
  { name: "teal", value: "#1abc9c" },
  { name: "blue", value: "#2980b9" },
  { name: "purple", value: "#8e44ad" },
  { name: "pink", value: "#e84393" },
];
const PRESET_TAGS = ["production", "staging", "development", "local", "test"];
let selectedColor = null;
let selectedTags = [];
let allExistingTags = [];

const colorPaletteEl = document.getElementById("color-palette");
const tagChipsEl = document.getElementById("tag-chips");
const tagInputEl = document.getElementById("tag-input");
const tagSuggestionsEl = document.getElementById("tag-suggestions");

function renderColorPalette() {
  colorPaletteEl.innerHTML = "";
  // None button
  const none = document.createElement("span");
  none.className = "color-dot-none" + (selectedColor == null ? " active" : "");
  none.textContent = "\u00D7";
  none.title = t('color_none');
  none.addEventListener("click", () => { selectedColor = null; renderColorPalette(); });
  colorPaletteEl.appendChild(none);
  // Color dots
  COLOR_PALETTE.forEach((c) => {
    const dot = document.createElement("span");
    dot.className = "color-dot" + (selectedColor === c.value ? " active" : "");
    dot.style.backgroundColor = c.value;
    dot.title = c.name;
    dot.addEventListener("click", () => {
      selectedColor = c.value;
      renderColorPalette();
    });
    colorPaletteEl.appendChild(dot);
  });
}

function renderTagChips() {
  tagChipsEl.innerHTML = "";
  selectedTags.forEach((tag) => {
    const chip = document.createElement("span");
    chip.className = "tag-chip";
    chip.textContent = tag;
    const rm = document.createElement("span");
    rm.className = "tag-chip-remove";
    rm.textContent = "\u00D7";
    rm.addEventListener("click", () => {
      selectedTags = selectedTags.filter((t) => t !== tag);
      renderTagChips();
    });
    chip.appendChild(rm);
    tagChipsEl.appendChild(chip);
  });
}

function addTag(tag) {
  const t = tag.trim().toLowerCase();
  if (!t || selectedTags.includes(t)) return;
  selectedTags.push(t);
  renderTagChips();
  tagInputEl.value = "";
  hideSuggestions();
}

function hideSuggestions() {
  tagSuggestionsEl.classList.add("hidden");
  tagSuggestionsEl.innerHTML = "";
  suggestIndex = -1;
}

let suggestIndex = -1;

function showSuggestions(query) {
  const q = query.trim().toLowerCase();
  // Merge preset + existing, deduplicate
  const pool = [...new Set([...PRESET_TAGS, ...allExistingTags])];
  const candidates = pool.filter((t) => !selectedTags.includes(t) && (!q || t.includes(q)));
  if (candidates.length === 0) { hideSuggestions(); return; }

  tagSuggestionsEl.innerHTML = "";
  suggestIndex = -1;
  candidates.forEach((t) => {
    const el = document.createElement("div");
    el.className = "tag-suggestion-item";
    el.textContent = t;
    el.addEventListener("mousedown", (e) => { e.preventDefault(); addTag(t); });
    tagSuggestionsEl.appendChild(el);
  });
  tagSuggestionsEl.classList.remove("hidden");
}

tagInputEl.addEventListener("input", () => {
  showSuggestions(tagInputEl.value);
});

tagInputEl.addEventListener("focus", () => {
  showSuggestions(tagInputEl.value);
});

tagInputEl.addEventListener("blur", () => {
  // Delay to allow mousedown on suggestion
  setTimeout(hideSuggestions, 150);
});

tagInputEl.addEventListener("keydown", (e) => {
  const items = tagSuggestionsEl.querySelectorAll(".tag-suggestion-item");

  if (e.key === "ArrowDown") {
    e.preventDefault();
    suggestIndex = Math.min(suggestIndex + 1, items.length - 1);
    items.forEach((el, i) => el.classList.toggle("active", i === suggestIndex));
  } else if (e.key === "ArrowUp") {
    e.preventDefault();
    suggestIndex = Math.max(suggestIndex - 1, 0);
    items.forEach((el, i) => el.classList.toggle("active", i === suggestIndex));
  } else if (e.key === "Enter") {
    e.preventDefault();
    if (suggestIndex >= 0 && items[suggestIndex]) {
      addTag(items[suggestIndex].textContent);
    } else if (tagInputEl.value.trim()) {
      addTag(tagInputEl.value);
    }
  } else if (e.key === ",") {
    e.preventDefault();
    if (tagInputEl.value.trim()) addTag(tagInputEl.value);
  } else if (e.key === "Backspace" && !tagInputEl.value) {
    selectedTags.pop();
    renderTagChips();
  }
});

async function loadAllExistingTags() {
  try {
    const data = await safeInvoke("list_profiles");
    const tags = new Set();
    data.items.forEach((item) => {
      if (item.tags) item.tags.forEach((t) => tags.add(t));
    });
    allExistingTags = [...tags];
  } catch (_) {}
}

renderColorPalette();
renderTagChips();

function show(value) {
  const text = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  resultEl.textContent = text;
  resultEl.hidden = !text;
}

async function loadSshConfigHosts() {
  try {
    const hosts = await safeInvoke("list_ssh_config_hosts");
    sshConfigHostSelect.innerHTML = '<option value="">' + t('config_host_manual') + '</option>';
    hosts.forEach((h) => {
      const opt = document.createElement("option");
      opt.value = h;
      opt.textContent = h;
      sshConfigHostSelect.appendChild(opt);
    });
  } catch (_) {}
}

function updateSshFieldVisibility() {
  const enabled = sshEnabledCheck.checked;
  // Disable all inputs/selects/buttons inside ssh-fields when SSH is off
  sshFieldsDiv.querySelectorAll("input, select, button").forEach((el) => {
    el.disabled = !enabled;
  });
  // Hide manual fields when Config Host is selected
  const useConfig = sshConfigHostSelect.value !== "";
  sshManualFieldIds.forEach((id) => {
    document.getElementById(id).closest("label").classList.toggle("hidden", useConfig);
  });
}

sshEnabledCheck.addEventListener("change", updateSshFieldVisibility);
sshConfigHostSelect.addEventListener("change", updateSshFieldVisibility);

function updateCaCertVisibility() {
  const mode = sslModeSelect.value;
  const show = mode === "VERIFY_CA" || mode === "VERIFY_IDENTITY";
  caCertRow.classList.toggle("hidden", !show);
}
sslModeSelect.addEventListener("change", updateCaCertVisibility);

async function browseFile(inputEl, title, filterName, extensions) {
  try {
    const path = await safeInvoke("pick_file", { title, filterName, extensions });
    if (path) inputEl.value = path;
  } catch (_) {}
}

caCertBrowseBtn.addEventListener("click", () => {
  browseFile(caCertInput, t('select_ca_cert'), "Certificate", ["pem", "crt", "cer"]);
});

sshKeyBrowseBtn.addEventListener("click", () => {
  browseFile(document.getElementById("ssh-key"), t('select_identity_file'), null, null);
});

function collectRequest() {
  const sshEnabled = document.getElementById("ssh-enabled").checked;

  return {
    mysql: {
      host: document.getElementById("mysql-host").value.trim(),
      port: Number(document.getElementById("mysql-port").value),
      database: document.getElementById("mysql-db").value.trim() || null,
      username: document.getElementById("mysql-user").value.trim(),
      password: document.getElementById("mysql-pass").value,
      ssl_mode: sslModeSelect.value,
      tls_ca_cert_path: caCertInput.value.trim() || null,
    },
    ssh: {
      enabled: sshEnabled,
      host: document.getElementById("ssh-host").value.trim(),
      port: Number(document.getElementById("ssh-port").value || 22),
      username: document.getElementById("ssh-user").value.trim(),
      private_key_path: document.getElementById("ssh-key").value.trim() || null,
      config_host: document.getElementById("ssh-config-host").value || null,
      passphrase: document.getElementById("ssh-passphrase").value,
    },
  };
}

function applyRequest(request) {
  document.getElementById("mysql-host").value = request.mysql.host || "";
  document.getElementById("mysql-port").value = request.mysql.port || 3306;
  document.getElementById("mysql-db").value = request.mysql.database || "";
  document.getElementById("mysql-user").value = request.mysql.username || "";
  document.getElementById("mysql-pass").value = request.mysql.password || "";
  sslModeSelect.value = request.mysql.ssl_mode || "DISABLED";
  caCertInput.value = request.mysql.tls_ca_cert_path || "";
  updateCaCertVisibility();

  const ssh = request.ssh || {
    enabled: false,
    host: "",
    port: 22,
    username: "",
    private_key_path: null,
  };
  document.getElementById("ssh-enabled").checked = !!ssh.enabled;
  document.getElementById("ssh-config-host").value = ssh.config_host || "";
  document.getElementById("ssh-host").value = ssh.host || "";
  document.getElementById("ssh-port").value = ssh.port || 22;
  document.getElementById("ssh-user").value = ssh.username || "";
  document.getElementById("ssh-key").value = ssh.private_key_path || "";
  document.getElementById("ssh-passphrase").value = ssh.passphrase || "";
  updateSshFieldVisibility();
}

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

menuBtn.addEventListener("click", () => safeInvoke("show_popup_menu", { lang: getLang(), theme: getTheme() }));

const mysqlPassInput = document.getElementById("mysql-pass");
const mysqlPassClearBtn = document.getElementById("mysql-pass-clear");
const sshPassphraseInput = document.getElementById("ssh-passphrase");
const sshPassphraseClearBtn = document.getElementById("ssh-passphrase-clear");
let clearPasswordFlag = false;
let clearSshPassphraseFlag = false;

mysqlPassClearBtn.addEventListener("click", () => {
  clearPasswordFlag = true;
  mysqlPassInput.value = "";
  mysqlPassInput.placeholder = t('password_cleared');
  mysqlPassClearBtn.classList.add("hidden");
});

mysqlPassInput.addEventListener("input", () => {
  if (clearPasswordFlag) {
    clearPasswordFlag = false;
  }
});

sshPassphraseClearBtn.addEventListener("click", () => {
  clearSshPassphraseFlag = true;
  sshPassphraseInput.value = "";
  sshPassphraseInput.placeholder = t('ssh_passphrase_cleared');
  sshPassphraseClearBtn.classList.add("hidden");
});

sshPassphraseInput.addEventListener("input", () => {
  if (clearSshPassphraseFlag) {
    clearSshPassphraseFlag = false;
  }
});

async function loadProfile(id) {
  const data = await safeInvoke("list_profiles");
  const profile = data.items.find((item) => item.id === id);
  if (!profile) {
    return null;
  }
  profileNameInput.value = profile.name;
  saveBtn.disabled = !profile.name.trim();
  selectedGroupId = profile.group_id || null;
  selectedOrder = profile.order || 0;
  selectedColor = profile.color || null;
  selectedTags = Array.isArray(profile.tags) ? [...profile.tags] : [];
  renderColorPalette();
  renderTagChips();
  applyRequest(profile.request);
  // Show placeholder and clear button if password is stored in keyring
  clearPasswordFlag = false;
  clearSshPassphraseFlag = false;
  try {
    const stored = await safeInvoke("has_password", { profileId: id });
    mysqlPassInput.placeholder = stored ? t('password_saved_placeholder') : "";
    mysqlPassClearBtn.classList.toggle("hidden", !stored);
  } catch (_) {
    mysqlPassInput.placeholder = "";
    mysqlPassClearBtn.classList.add("hidden");
  }
  // Show placeholder and clear button if SSH passphrase is stored in keyring
  try {
    const sshPpStored = await safeInvoke("has_ssh_passphrase", { profileId: id });
    sshPassphraseInput.placeholder = sshPpStored ? t('ssh_passphrase_saved_placeholder') : "";
    sshPassphraseClearBtn.classList.toggle("hidden", !sshPpStored);
  } catch (_) {
    sshPassphraseInput.placeholder = "";
    sshPassphraseClearBtn.classList.add("hidden");
  }
  return profile;
}

function clearForm() {
  profileNameInput.value = "";
  mysqlPassInput.placeholder = "";
  mysqlPassClearBtn.classList.add("hidden");
  clearPasswordFlag = false;
  sshPassphraseInput.placeholder = "";
  sshPassphraseClearBtn.classList.add("hidden");
  clearSshPassphraseFlag = false;
  selectedColor = null;
  selectedTags = [];
  renderColorPalette();
  renderTagChips();
  applyRequest({
    mysql: {
      host: "127.0.0.1",
      port: 3306,
      database: null,
      username: "root",
      password: "",
      ssl_mode: "DISABLED",
      tls_ca_cert_path: null,
    },
    ssh: {
      enabled: false,
      host: "",
      port: 22,
      username: "",
      private_key_path: null,
      config_host: null,
      passphrase: "",
    },
  });
}

testBtn.addEventListener("click", async () => {
  try {
    show(t('connecting'));
    const request = collectRequest();
    const res = await safeInvoke("test_connection", { request, profileId: selectedProfileId || null });
    show(res);
  } catch (error) {
    show(String(error));
  }
});

saveBtn.addEventListener("click", async () => {
  try {
    const name = profileNameInput.value.trim();
    if (!name) {
      show(t('profile_name_required'));
      return;
    }
    const profile = {
      id: selectedProfileId || "",
      name,
      group_id: selectedGroupId,
      order: selectedOrder,
      color: selectedColor || null,
      tags: selectedTags,
      request: collectRequest(),
      clear_password: clearPasswordFlag,
      clear_ssh_passphrase: clearSshPassphraseFlag,
    };
    await safeInvoke("save_profile", { profile });
    if (eventApi && eventApi.emit) {
      await eventApi.emit("profiles:changed");
    }
    await safeInvoke("hide_window");
  } catch (error) {
    show(String(error));
  }
});

deleteBtn.addEventListener("click", async () => {
  if (!selectedProfileId) {
    return;
  }
  try {
    await safeInvoke("delete_profile", { id: selectedProfileId });
    if (eventApi && eventApi.emit) {
      await eventApi.emit("profiles:changed");
    }
    await safeInvoke("hide_window");
  } catch (error) {
    show(String(error));
  }
});

connectBtn.addEventListener("click", async () => {
  if (!selectedProfileId) return;
  try {
    await safeInvoke("open_query_window", { id: selectedProfileId });
    await safeInvoke("hide_window");
  } catch (error) {
    show(String(error));
  }
});

cancelBtn.addEventListener("click", async () => {
  await safeInvoke("hide_window");
});

profileNameInput.addEventListener("input", () => {
  saveBtn.disabled = !profileNameInput.value.trim();
});

clearForm();
saveBtn.disabled = true;
deleteBtn.disabled = true;
connectBtn.disabled = true;

if (eventApi && eventApi.listen) {
  eventApi.listen("menu:action", (event) => {
    switch (event.payload) {
      case "test-connection": testBtn.click(); break;
      case "connect": connectBtn.click(); break;
      case "save": saveBtn.click(); break;
      case "delete": deleteBtn.click(); break;
      case "theme-light": setTheme("light"); break;
      case "theme-dark": setTheme("dark"); break;
      case "lang-en": setLang("en"); break;
      case "lang-ja": setLang("ja"); break;
    }
  });
  eventApi.listen("settings:open", (event) => {
    const payload = event.payload || {};
    const id = (typeof payload === "string") ? payload : (payload.id || "");
    const groupId = (typeof payload === "object" && payload !== null) ? (payload.group_id || null) : null;
    selectedProfileId = id;
    deleteBtn.disabled = !selectedProfileId;
    connectBtn.disabled = !selectedProfileId;
    show("");
    Promise.all([loadSshConfigHosts(), loadAllExistingTags()]).then(() => {
      if (selectedProfileId) {
        loadProfile(selectedProfileId).catch((error) => show(String(error)));
      } else {
        clearForm();
        selectedGroupId = groupId;
      }
    });
  });
}

// Re-render on language change
window.addEventListener("musql:langchange", () => {
  applySettingsLabels();
  applyI18n();
});
