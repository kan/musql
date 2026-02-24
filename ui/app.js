const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;

const resultEl = document.getElementById("result");
const profileTree = document.getElementById("profile-tree");
const profileNameInput = document.getElementById("profile-name");
const profileNewBtn = document.getElementById("profile-new");
const profileSaveBtn = document.getElementById("profile-save");
const profileDeleteBtn = document.getElementById("profile-delete");

let profiles = [];
let selectedProfileId = null;

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

function show(value) {
  resultEl.textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

function groupProfiles(items) {
  const groups = new Map();
  const ungrouped = [];
  items.forEach((item) => {
    const parts = item.name.split("/");
    if (parts.length > 1) {
      const group = parts.shift();
      const label = parts.join("/");
      if (!groups.has(group)) {
        groups.set(group, []);
      }
      groups.get(group).push({ ...item, displayName: label });
    } else {
      ungrouped.push({ ...item, displayName: item.name });
    }
  });
  return { groups, ungrouped };
}

function renderProfiles() {
  profileTree.innerHTML = "";
  const { groups, ungrouped } = groupProfiles(profiles);

  ungrouped.forEach((item) => {
    const div = document.createElement("div");
    div.className = `tree-item${item.id === selectedProfileId ? " active" : ""}`;
    div.textContent = item.displayName;
    div.addEventListener("click", () => selectProfile(item.id));
    profileTree.appendChild(div);
  });

  groups.forEach((items, groupName) => {
    const group = document.createElement("div");
    group.className = "tree-group";
    const title = document.createElement("h3");
    title.textContent = groupName;
    group.appendChild(title);
    items.forEach((item) => {
      const div = document.createElement("div");
      div.className = `tree-item${item.id === selectedProfileId ? " active" : ""}`;
      div.textContent = item.displayName;
      div.addEventListener("click", () => selectProfile(item.id));
      group.appendChild(div);
    });
    profileTree.appendChild(group);
  });

  profileDeleteBtn.disabled = !selectedProfileId;
}

function selectProfile(id) {
  const profile = profiles.find((item) => item.id === id);
  if (!profile) {
    return;
  }
  selectedProfileId = id;
  profileNameInput.value = profile.name;
  applyRequest(profile.request);
  renderProfiles();
}

function resetProfileSelection() {
  selectedProfileId = null;
  profileNameInput.value = "";
  renderProfiles();
}

async function refreshProfiles() {
  profiles = await safeInvoke("list_profiles");
  renderProfiles();
}

document.getElementById("test-btn").addEventListener("click", async () => {
  try {
    show("connecting...");
    const request = collectRequest();
    const res = await safeInvoke("test_connection", { request });
    show(res);
  } catch (error) {
    show(String(error));
  }
});

document.getElementById("run-btn").addEventListener("click", async () => {
  try {
    show("running...");
    const request = collectRequest();
    const query = document.getElementById("query").value;
    const res = await safeInvoke("run_query", { request, query });
    show(res);
  } catch (error) {
    show(String(error));
  }
});

profileNewBtn.addEventListener("click", () => {
  resetProfileSelection();
});

profileSaveBtn.addEventListener("click", async () => {
  try {
    const name = profileNameInput.value.trim();
    if (!name) {
      show("profile name is required");
      return;
    }
    const request = collectRequest();
    const profile = {
      id: selectedProfileId || "",
      name,
      request,
    };
    profiles = await safeInvoke("save_profile", { profile });
    if (profile.id) {
      selectedProfileId = profile.id;
    } else {
      const candidates = profiles.filter((item) => item.name === name);
      if (candidates.length > 0) {
        selectedProfileId = candidates[candidates.length - 1].id;
      }
    }
    renderProfiles();
  } catch (error) {
    show(String(error));
  }
});

profileDeleteBtn.addEventListener("click", async () => {
  if (!selectedProfileId) {
    return;
  }
  try {
    profiles = await safeInvoke("delete_profile", { id: selectedProfileId });
    resetProfileSelection();
  } catch (error) {
    show(String(error));
  }
});

refreshProfiles().catch((error) => show(String(error)));
