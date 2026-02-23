const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;

const resultEl = document.getElementById("result");

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

function show(value) {
  resultEl.textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
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
