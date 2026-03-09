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
const sshConfigRow = document.getElementById("ssh-config-row");
const sshEnabledCheck = document.getElementById("ssh-enabled");
const sshFieldsDiv = document.getElementById("ssh-fields");
const sshManualFieldIds = ["ssh-host", "ssh-port", "ssh-user", "ssh-key", "ssh-password"];
let sshConfigHostValue = ""; // current config host alias (empty = manual)
const sslModeSelect = document.getElementById("mysql-ssl-mode");
const caCertRow = document.getElementById("ca-cert-row");
const caCertInput = document.getElementById("mysql-ca-cert");
const caCertBrowseBtn = document.getElementById("mysql-ca-cert-browse");
const sshKeyBrowseBtn = document.getElementById("ssh-key-browse");
const sshAuthMethodSelect = document.getElementById("ssh-auth-method");
const sshKeyRow = document.getElementById("ssh-key-row");
const sshPassphraseRow = document.getElementById("ssh-passphrase-row");
const sshPasswordRow = document.getElementById("ssh-password-row");
const sshPasswordInput = document.getElementById("ssh-password");
const sshPasswordClearBtn = document.getElementById("ssh-password-clear");
const mysqlSavePasswordCheck = document.getElementById("mysql-save-password");
const sshSavePassphraseCheck = document.getElementById("ssh-save-passphrase");
const sshSavePasswordCheck = document.getElementById("ssh-save-password");

const menuBtn = document.getElementById("menu-btn");
menuBtn.innerHTML = icon('menu');

// Apply icons to buttons
function applySettingsLabels() {
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

const colorSelectBtn = document.getElementById("color-select-btn");
const colorSelectDropdown = document.getElementById("color-select-dropdown");
const tagChipsEl = document.getElementById("tag-chips");
const tagInputEl = document.getElementById("tag-input");
const tagSuggestionsEl = document.getElementById("tag-suggestions");

function renderColorSelect() {
  // Update button label
  updateColorBtn();
  // Build dropdown items
  colorSelectDropdown.innerHTML = "";
  // None item
  const noneItem = document.createElement("div");
  noneItem.className = "color-select-item" + (selectedColor == null ? " active" : "");
  noneItem.textContent = t('color_none');
  noneItem.addEventListener("click", () => { selectedColor = null; closeColorSelect(); renderColorSelect(); });
  colorSelectDropdown.appendChild(noneItem);
  // Color items
  COLOR_PALETTE.forEach((c) => {
    const item = document.createElement("div");
    item.className = "color-select-item" + (selectedColor === c.value ? " active" : "");
    const swatch = document.createElement("span");
    swatch.className = "color-swatch";
    swatch.style.backgroundColor = c.value;
    item.appendChild(swatch);
    item.appendChild(document.createTextNode(c.name));
    item.addEventListener("click", () => { selectedColor = c.value; closeColorSelect(); renderColorSelect(); });
    colorSelectDropdown.appendChild(item);
  });
}

function updateColorBtn() {
  colorSelectBtn.innerHTML = "";
  if (selectedColor) {
    const swatch = document.createElement("span");
    swatch.className = "color-swatch";
    swatch.style.backgroundColor = selectedColor;
    colorSelectBtn.appendChild(swatch);
    const entry = COLOR_PALETTE.find((c) => c.value === selectedColor);
    colorSelectBtn.appendChild(document.createTextNode(entry ? entry.name : selectedColor));
  } else {
    colorSelectBtn.appendChild(document.createTextNode(t('color_none')));
  }
}

function closeColorSelect() {
  colorSelectDropdown.classList.add("hidden");
}

colorSelectBtn.addEventListener("click", (e) => {
  e.stopPropagation();
  colorSelectDropdown.classList.toggle("hidden");
});

document.addEventListener("click", (e) => {
  if (!e.target.closest("#color-select")) closeColorSelect();
});

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeColorSelect();
});

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

renderColorSelect();
renderTagChips();

function show(value) {
  const text = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  resultEl.textContent = text;
  resultEl.hidden = !text;
}

let sshConfigHosts = []; // cached list

async function loadSshConfigHosts() {
  try {
    sshConfigHosts = await safeInvoke("list_ssh_config_hosts");
  } catch (_) {
    sshConfigHosts = [];
  }
}

function renderSshConfigRow() {
  sshConfigRow.innerHTML = "";
  const enabled = sshEnabledCheck.checked;

  if (!sshConfigHostValue) {
    // Initial state: show "Load from ssh config" button
    const loadBtn = document.createElement("button");
    loadBtn.className = "ghost";
    loadBtn.textContent = t('ssh_config_load');
    loadBtn.disabled = !enabled;
    loadBtn.addEventListener("click", showSshConfigModal);
    sshConfigRow.appendChild(loadBtn);
  } else {
    // Active state: show reference text + clear button
    const refText = document.createElement("span");
    refText.className = "ssh-config-ref";
    refText.textContent = t('ssh_config_ref', { name: sshConfigHostValue });
    sshConfigRow.appendChild(refText);
    const clearBtn = document.createElement("button");
    clearBtn.className = "ghost danger";
    clearBtn.textContent = t('ssh_config_clear');
    clearBtn.disabled = !enabled;
    clearBtn.addEventListener("click", clearSshConfig);
    sshConfigRow.appendChild(clearBtn);
  }
}

function showSshConfigModal() {
  const overlay = document.createElement("div");
  overlay.className = "modal-overlay";
  const box = document.createElement("div");
  box.className = "modal-box";
  const heading = document.createElement("h2");
  heading.textContent = t('ssh_config_select');
  box.appendChild(heading);

  if (sshConfigHosts.length === 0) {
    const empty = document.createElement("p");
    empty.className = "modal-status";
    empty.textContent = "No hosts found in ~/.ssh/config";
    box.appendChild(empty);
  } else {
    const list = document.createElement("div");
    list.className = "db-list";
    sshConfigHosts.forEach((host) => {
      const el = document.createElement("div");
      el.className = "db-list-item";
      el.textContent = host;
      el.addEventListener("click", () => {
        close();
        selectSshConfigHost(host);
      });
      list.appendChild(el);
    });
    box.appendChild(list);
  }

  overlay.appendChild(box);
  document.body.appendChild(overlay);

  function close() {
    overlay.remove();
    document.removeEventListener("keydown", onKey);
  }
  function onKey(e) { if (e.key === "Escape") close(); }
  document.addEventListener("keydown", onKey);
  overlay.addEventListener("click", (e) => { if (e.target === overlay) close(); });
}

async function selectSshConfigHost(alias) {
  sshConfigHostValue = alias;
  renderSshConfigRow();
  updateSshFieldVisibility();
  try {
    const resolved = await safeInvoke("resolve_ssh_config", { alias });
    document.getElementById("ssh-host").value = resolved.host || "";
    document.getElementById("ssh-port").value = resolved.port || 22;
    document.getElementById("ssh-user").value = resolved.user || "";
    document.getElementById("ssh-key").value = resolved.identity_file || "";
  } catch (_) {}
}

function clearSshConfig() {
  sshConfigHostValue = "";
  document.getElementById("ssh-host").value = "";
  document.getElementById("ssh-port").value = "22";
  document.getElementById("ssh-user").value = "";
  document.getElementById("ssh-key").value = "";
  renderSshConfigRow();
  updateSshFieldVisibility();
}

function updateSshFieldVisibility() {
  const enabled = sshEnabledCheck.checked;
  const useConfig = sshConfigHostValue !== "";

  // Disable all inputs/buttons inside ssh-fields when SSH is off
  sshFieldsDiv.querySelectorAll("input, select, button").forEach((el) => {
    el.disabled = !enabled;
  });
  // When config host is set: force auth_method to "key", disable manual fields + auth method
  if (useConfig && enabled) {
    sshAuthMethodSelect.value = "key";
    sshAuthMethodSelect.disabled = true;
    updateSshAuthMethodVisibility();
    sshManualFieldIds.forEach((id) => {
      const el = document.getElementById(id);
      const label = el.closest("label");
      el.disabled = true;
      label.querySelectorAll("button").forEach((btn) => { btn.disabled = true; });
    });
  }
  // Also update the config row buttons
  renderSshConfigRow();
}

sshEnabledCheck.addEventListener("change", updateSshFieldVisibility);

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
      save_password: mysqlSavePasswordCheck.checked,
    },
    ssh: {
      enabled: sshEnabled,
      host: document.getElementById("ssh-host").value.trim(),
      port: Number(document.getElementById("ssh-port").value || 22),
      username: document.getElementById("ssh-user").value.trim(),
      private_key_path: document.getElementById("ssh-key").value.trim() || null,
      config_host: sshConfigHostValue || null,
      passphrase: document.getElementById("ssh-passphrase").value,
      auth_method: sshAuthMethodSelect.value,
      ssh_password: sshPasswordInput.value,
      save_ssh_password: sshSavePasswordCheck.checked,
      save_ssh_passphrase: sshSavePassphraseCheck.checked,
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
  mysqlSavePasswordCheck.checked = request.mysql.save_password !== false;
  updateCaCertVisibility();

  const ssh = request.ssh || {
    enabled: false,
    host: "",
    port: 22,
    username: "",
    private_key_path: null,
  };
  document.getElementById("ssh-enabled").checked = !!ssh.enabled;
  sshConfigHostValue = ssh.config_host || "";
  document.getElementById("ssh-host").value = ssh.host || "";
  document.getElementById("ssh-port").value = ssh.port || 22;
  document.getElementById("ssh-user").value = ssh.username || "";
  document.getElementById("ssh-key").value = ssh.private_key_path || "";
  document.getElementById("ssh-passphrase").value = ssh.passphrase || "";
  sshAuthMethodSelect.value = ssh.auth_method || "key";
  sshPasswordInput.value = ssh.ssh_password || "";
  sshSavePasswordCheck.checked = ssh.save_ssh_password !== false;
  sshSavePassphraseCheck.checked = ssh.save_ssh_passphrase !== false;
  renderSshConfigRow();
  updateSshFieldVisibility();
  updateSshAuthMethodVisibility();

  // If config_host is set, resolve and populate SSH fields
  if (ssh.config_host) {
    safeInvoke("resolve_ssh_config", { alias: ssh.config_host }).then((resolved) => {
      document.getElementById("ssh-host").value = resolved.host || "";
      document.getElementById("ssh-port").value = resolved.port || 22;
      document.getElementById("ssh-user").value = resolved.user || "";
      document.getElementById("ssh-key").value = resolved.identity_file || "";
    }).catch(() => {});
  }
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
let clearSshPasswordFlag = false;

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

sshPasswordClearBtn.addEventListener("click", () => {
  clearSshPasswordFlag = true;
  sshPasswordInput.value = "";
  sshPasswordInput.placeholder = t('ssh_password_cleared');
  sshPasswordClearBtn.classList.add("hidden");
});

sshPasswordInput.addEventListener("input", () => {
  if (clearSshPasswordFlag) {
    clearSshPasswordFlag = false;
  }
});

function updateSshAuthMethodVisibility() {
  const method = sshAuthMethodSelect.value;
  const isKey = method === "key";
  sshKeyRow.classList.toggle("hidden", !isKey);
  sshPassphraseRow.classList.toggle("hidden", !isKey);
  sshPasswordRow.classList.toggle("hidden", isKey);
}
sshAuthMethodSelect.addEventListener("change", updateSshAuthMethodVisibility);

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
  renderColorSelect();
  renderTagChips();
  applyRequest(profile.request);
  // Show placeholder and clear button if password is stored in keyring
  clearPasswordFlag = false;
  clearSshPassphraseFlag = false;
  clearSshPasswordFlag = false;
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
  // Show placeholder and clear button if SSH password is stored in keyring
  try {
    const sshPwStored = await safeInvoke("has_ssh_password", { profileId: id });
    sshPasswordInput.placeholder = sshPwStored ? t('ssh_password_saved_placeholder') : "";
    sshPasswordClearBtn.classList.toggle("hidden", !sshPwStored);
  } catch (_) {
    sshPasswordInput.placeholder = "";
    sshPasswordClearBtn.classList.add("hidden");
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
  sshPasswordInput.placeholder = "";
  sshPasswordClearBtn.classList.add("hidden");
  clearSshPasswordFlag = false;
  selectedColor = null;
  selectedTags = [];
  renderColorSelect();
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
      save_password: true,
    },
    ssh: {
      enabled: false,
      host: "",
      port: 22,
      username: "",
      private_key_path: null,
      config_host: null,
      passphrase: "",
      auth_method: "key",
      ssh_password: "",
      save_ssh_password: true,
      save_ssh_passphrase: true,
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
      clear_ssh_password: clearSshPasswordFlag,
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
  try {
    const name = profileNameInput.value.trim();
    if (!name) {
      show(t('profile_name_required'));
      return;
    }
    // Save profile first (same logic as save button)
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
      clear_ssh_password: clearSshPasswordFlag,
    };
    const result = await safeInvoke("save_profile", { profile });
    if (eventApi && eventApi.emit) {
      await eventApi.emit("profiles:changed");
    }
    const profileId = result.saved_id || selectedProfileId;
    await safeInvoke("open_query_window", { id: profileId });
    await safeInvoke("hide_window");
  } catch (error) {
    show(String(error));
  }
});

cancelBtn.addEventListener("click", async () => {
  await safeInvoke("hide_window");
});

profileNameInput.addEventListener("input", () => {
  const hasName = !!profileNameInput.value.trim();
  saveBtn.disabled = !hasName;
  connectBtn.disabled = !hasName;
});

clearForm();
renderSshConfigRow();
saveBtn.disabled = true;
deleteBtn.disabled = true;
connectBtn.disabled = true; // enabled when profile name is entered

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
    show("");
    Promise.all([loadSshConfigHosts(), loadAllExistingTags()]).then(() => {
      if (selectedProfileId) {
        loadProfile(selectedProfileId).then(() => {
          connectBtn.disabled = !profileNameInput.value.trim();
        }).catch((error) => show(String(error)));
      } else {
        clearForm();
        selectedGroupId = groupId;
        connectBtn.disabled = true;
      }
    });
  });
}

// Re-render on language change
window.addEventListener("musql:langchange", () => {
  applySettingsLabels();
  applyI18n();
});
