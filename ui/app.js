const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;

const profileList = document.getElementById("profile-list");
const profileNewBtn = document.getElementById("profile-new");

let profiles = [];
async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

function renderProfiles() {
  profileList.innerHTML = "";
  profiles.forEach((item) => {
    const row = document.createElement("div");
    row.className = "list-item";

    const meta = document.createElement("div");
    meta.className = "meta";
    const name = document.createElement("div");
    name.className = "name";
    name.textContent = item.name;
    const hint = document.createElement("div");
    hint.className = "hint";
    const ssh = item.request.ssh;
    const mysqlHost = item.request.mysql.host || "host not set";
    hint.textContent = ssh && ssh.enabled && ssh.host
      ? ssh.host + " → " + mysqlHost
      : mysqlHost;
    meta.appendChild(name);
    meta.appendChild(hint);

    const buttons = document.createElement("div");
    buttons.className = "buttons";
    const openBtn = document.createElement("button");
    openBtn.textContent = "開く";
    openBtn.addEventListener("click", () => openQuery(item.id));
    const settingsBtn = document.createElement("button");
    settingsBtn.textContent = "設定";
    settingsBtn.addEventListener("click", () => openSettings(item.id));
    buttons.appendChild(openBtn);
    buttons.appendChild(settingsBtn);

    row.appendChild(meta);
    row.appendChild(buttons);
    profileList.appendChild(row);
  });
}

async function refreshProfiles() {
  profiles = await safeInvoke("list_profiles");
  renderProfiles();
}

function openSettings(id) {
  safeInvoke("open_settings_window", { id: id || null })
    .then(() => {})
    .catch((error) => alert(String(error)));
}

function openQuery(id) {
  safeInvoke("open_query_window", { id })
    .then(() => {})
    .catch((error) => alert(String(error)));
}

profileNewBtn.addEventListener("click", () => openSettings(""));

refreshProfiles().catch((error) => alert(String(error)));
window.addEventListener("focus", () => refreshProfiles());
