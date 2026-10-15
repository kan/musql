// The credential prompt shared by the connection windows (query, Redis #98): asks for
// the passwords a profile does not keep (`save_*` is false) before connecting. The page
// holds the modal (#pw-prompt-modal and its parts). Needs i18n.js (`t`).

// Cancels the prompt on screen, if any (its promise resolves false). For a window that is
// closed while the prompt is open: otherwise the prompt stays and answers for the next open.
let cancelOpenPrompt = null;
function cancelCredentialPrompt() {
  if (cancelOpenPrompt) cancelOpenPrompt();
}

// Resolves true once the fields are filled in (they are written into `request`), or
// false when the user cancels. Resolves true at once when nothing needs asking.
function promptForCredentials(request) {
  // An earlier prompt still open is cancelled first: its cleanup hides the modal, which
  // must happen before this prompt fills and shows it.
  cancelCredentialPrompt();
  const modal = document.getElementById("pw-prompt-modal");
  const fieldsEl = document.getElementById("pw-prompt-fields");
  const errorEl = document.getElementById("pw-prompt-error");
  const okBtn = document.getElementById("pw-prompt-ok");
  const cancelBtn = document.getElementById("pw-prompt-cancel");

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

    fieldsEl.innerHTML = "";
    errorEl.textContent = "";
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
      fieldsEl.appendChild(lbl);
      inputs[f.key] = inp;
    });

    modal.classList.remove("hidden");
    const firstInput = Object.values(inputs)[0];
    if (firstInput) firstInput.focus();

    function cleanup() {
      cancelOpenPrompt = null;
      modal.classList.add("hidden");
      okBtn.removeEventListener("click", onOk);
      cancelBtn.removeEventListener("click", onCancel);
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
    cancelOpenPrompt = onCancel;
    okBtn.addEventListener("click", onOk);
    cancelBtn.addEventListener("click", onCancel);
    document.addEventListener("keydown", onKey);
  });
}

window.promptForCredentials = promptForCredentials;
window.cancelCredentialPrompt = cancelCredentialPrompt;
