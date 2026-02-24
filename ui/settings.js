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
const sshConfigHostSelect = document.getElementById("ssh-config-host");
const sshEnabledCheck = document.getElementById("ssh-enabled");
const sshFieldsDiv = document.getElementById("ssh-fields");
const sshManualFieldIds = ["ssh-host", "ssh-port", "ssh-user", "ssh-key"];
const sslModeSelect = document.getElementById("mysql-ssl-mode");
const caCertRow = document.getElementById("ca-cert-row");
const caCertInput = document.getElementById("mysql-ca-cert");
const caCertBrowseBtn = document.getElementById("mysql-ca-cert-browse");
const sshKeyBrowseBtn = document.getElementById("ssh-key-browse");

let selectedProfileId = "";
let selectedGroupId = null;
let selectedOrder = 0;

function show(value) {
  const text = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  resultEl.textContent = text;
  resultEl.hidden = !text;
}

async function loadSshConfigHosts() {
  try {
    const hosts = await safeInvoke("list_ssh_config_hosts");
    sshConfigHostSelect.innerHTML = '<option value="">(manual)</option>';
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
  browseFile(caCertInput, "Select CA Certificate", "Certificate", ["pem", "crt", "cer"]);
});

sshKeyBrowseBtn.addEventListener("click", () => {
  browseFile(document.getElementById("ssh-key"), "Select Identity File", null, null);
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
  updateSshFieldVisibility();
}

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

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
  applyRequest(profile.request);
  return profile;
}

function clearForm() {
  profileNameInput.value = "";
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
    },
  });
}

testBtn.addEventListener("click", async () => {
  try {
    show("connecting...");
    const request = collectRequest();
    const res = await safeInvoke("test_connection", { request });
    show(res);
  } catch (error) {
    show(String(error));
  }
});

saveBtn.addEventListener("click", async () => {
  try {
    const name = profileNameInput.value.trim();
    if (!name) {
      show("profile name is required");
      return;
    }
    const profile = {
      id: selectedProfileId || "",
      name,
      group_id: selectedGroupId,
      order: selectedOrder,
      request: collectRequest(),
    };
    await safeInvoke("save_profile", { profile });
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

if (eventApi && eventApi.listen) {
  eventApi.listen("settings:open", (event) => {
    const id = event.payload || "";
    selectedProfileId = id;
    deleteBtn.disabled = !selectedProfileId;
    show("");
    loadSshConfigHosts().then(() => {
      if (selectedProfileId) {
        loadProfile(selectedProfileId).catch((error) => show(String(error)));
      } else {
        clearForm();
      }
    });
  });
}
