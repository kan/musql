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

let selectedProfileId = "";
let selectedGroupId = null;
let selectedOrder = 0;

function show(value) {
  const text = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  resultEl.textContent = text;
  resultEl.hidden = !text;
}

function collectRequest() {
  const sshEnabled = document.getElementById("ssh-enabled").checked;

  return {
    mysql: {
      host: document.getElementById("mysql-host").value.trim(),
      port: Number(document.getElementById("mysql-port").value),
      database: document.getElementById("mysql-db").value.trim() || null,
      username: document.getElementById("mysql-user").value.trim(),
      password: document.getElementById("mysql-pass").value,
      tls_enabled: document.getElementById("mysql-tls").checked,
      tls_skip_verify: document.getElementById("mysql-tls-skip").checked,
    },
    ssh: {
      enabled: sshEnabled,
      host: document.getElementById("ssh-host").value.trim(),
      port: Number(document.getElementById("ssh-port").value || 22),
      username: document.getElementById("ssh-user").value.trim(),
      private_key_path: document.getElementById("ssh-key").value.trim() || null,
    },
  };
}

function applyRequest(request) {
  document.getElementById("mysql-host").value = request.mysql.host || "";
  document.getElementById("mysql-port").value = request.mysql.port || 3306;
  document.getElementById("mysql-db").value = request.mysql.database || "";
  document.getElementById("mysql-user").value = request.mysql.username || "";
  document.getElementById("mysql-pass").value = request.mysql.password || "";
  document.getElementById("mysql-tls").checked = !!request.mysql.tls_enabled;
  document.getElementById("mysql-tls-skip").checked = !!request.mysql.tls_skip_verify;

  const ssh = request.ssh || {
    enabled: false,
    host: "",
    port: 22,
    username: "",
    private_key_path: null,
  };
  document.getElementById("ssh-enabled").checked = !!ssh.enabled;
  document.getElementById("ssh-host").value = ssh.host || "";
  document.getElementById("ssh-port").value = ssh.port || 22;
  document.getElementById("ssh-user").value = ssh.username || "";
  document.getElementById("ssh-key").value = ssh.private_key_path || "";
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
      tls_enabled: false,
      tls_skip_verify: false,
    },
    ssh: {
      enabled: false,
      host: "",
      port: 22,
      username: "",
      private_key_path: null,
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

clearForm();
deleteBtn.disabled = true;

if (eventApi && eventApi.listen) {
  eventApi.listen("settings:open", (event) => {
    const id = event.payload || "";
    selectedProfileId = id;
    deleteBtn.disabled = !selectedProfileId;
    show("");
    if (selectedProfileId) {
      loadProfile(selectedProfileId).catch((error) => show(String(error)));
    } else {
      clearForm();
    }
  });
}
