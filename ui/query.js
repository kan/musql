const invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
  ? window.__TAURI__.core.invoke
  : null;
const eventApi = window.__TAURI__ && window.__TAURI__.event ? window.__TAURI__.event : null;

const resultEl = document.getElementById("result");
const runBtn = document.getElementById("run-btn");

let requestCache = null;

function show(value) {
  resultEl.textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

async function safeInvoke(command, payload) {
  if (!invoke) {
    throw new Error("Tauri API unavailable. Run this page via `cargo tauri dev`.");
  }
  return invoke(command, payload);
}

async function loadProfile(id) {
  const items = await safeInvoke("list_profiles");
  const profile = items.find((item) => item.id === id);
  if (!profile) {
    throw new Error("profile not found");
  }
  requestCache = profile.request;
}

runBtn.addEventListener("click", async () => {
  try {
    show("running...");
    const query = document.getElementById("query").value;
    const res = await safeInvoke("run_query", { request: requestCache, query });
    show(res);
  } catch (error) {
    show(String(error));
  }
});

show("profile not selected");

if (eventApi && eventApi.listen) {
  eventApi.listen("query:open", (event) => {
    const id = event.payload || "";
    if (!id) {
      show("profile id missing");
      requestCache = null;
      return;
    }
    loadProfile(id).then(() => show("ready")).catch((error) => show(String(error)));
  });
}
