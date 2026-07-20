#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mysql::{prelude::Queryable, OptsBuilder, Pool, Row, SslOpts, Value};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, EventTarget, Manager, Window, Wry};

#[cfg(feature = "docker")]
mod docker;
mod onepassword;

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MySqlConfig {
    host: String,
    port: u16,
    database: Option<String>,
    username: String,
    #[serde(default, skip_serializing)]
    password: String,
    #[serde(default = "default_ssl_mode")]
    ssl_mode: String, // "DISABLED" | "REQUIRED" | "VERIFY_CA" | "VERIFY_IDENTITY"
    #[serde(default)]
    tls_ca_cert_path: Option<String>,
    #[serde(default = "default_true")]
    save_password: bool, // false = don't persist to keyring (prompt each time)
    // 1Password secret reference (op://vault/item/field) used to seed / refresh the
    // keyring entry. A pointer, not a secret, so it syncs with the profile (#80).
    #[serde(default)]
    op_ref: Option<String>,
    // Legacy (read-only, never re-saved)
    #[serde(default, skip_serializing)]
    tls_enabled: bool,
    #[serde(default, skip_serializing)]
    tls_skip_verify: bool,
}

fn default_ssl_mode() -> String {
    "DISABLED".to_string()
}

fn default_ssh_auth_method() -> String {
    "key".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct SshConfig {
    enabled: bool,
    host: String,
    port: u16,
    username: String,
    private_key_path: Option<String>,
    #[serde(default)]
    config_host: Option<String>,
    #[serde(default, skip_serializing)]
    passphrase: String,
    #[serde(default = "default_ssh_auth_method")]
    auth_method: String, // "key" | "password"
    #[serde(default, skip_serializing)]
    ssh_password: String, // password auth (transient)
    #[serde(default = "default_true")]
    save_ssh_password: bool, // false = don't persist to keyring
    #[serde(default = "default_true")]
    save_ssh_passphrase: bool, // false = don't persist to keyring
    // 1Password secret references (see MySqlConfig::op_ref)
    #[serde(default)]
    op_passphrase_ref: Option<String>,
    #[serde(default)]
    op_password_ref: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ConnectionRequest {
    mysql: MySqlConfig,
    ssh: Option<SshConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ProfileGroup {
    id: String,
    name: String,
    order: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ConnectionProfile {
    id: String,
    name: String,
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    order: u32,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    request: ConnectionRequest,
    #[serde(default, skip_serializing)]
    clear_password: bool,
    #[serde(default, skip_serializing)]
    clear_ssh_passphrase: bool,
    #[serde(default, skip_serializing)]
    clear_ssh_password: bool,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct ConnectionProfileStore {
    version: u32,
    #[serde(default)]
    groups: Vec<ProfileGroup>,
    items: Vec<ConnectionProfile>,
}

#[derive(Debug, Serialize)]
struct ProfileListResponse {
    groups: Vec<ProfileGroup>,
    items: Vec<ConnectionProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    saved_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportData {
    version: u32,
    groups: Vec<ProfileGroup>,
    items: Vec<ConnectionProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    passwords: Option<std::collections::HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ssh_passphrases: Option<std::collections::HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
struct ImportConflicts {
    groups: Vec<String>,   // duplicate group names
    profiles: Vec<String>, // duplicate profile names
}

#[derive(Debug, Serialize)]
struct ImportResult {
    groups: Vec<ProfileGroup>,
    items: Vec<ConnectionProfile>,
    imported_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflicts: Option<ImportConflicts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct QueryResult {
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
    affected_rows: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AiProvider {
    Claude,
    #[serde(alias = "openai")]
    OpenAi,
    Gemini,
}

#[derive(Debug, Clone, Serialize)]
struct SchemaColumn {
    name: String,
    data_type: String,
    column_key: String,
    is_nullable: String,
}

#[derive(Debug, Clone, Serialize)]
struct SchemaTable {
    name: String,
    columns: Vec<SchemaColumn>,
}

#[derive(Debug, Clone, Serialize)]
struct SchemaInfo {
    database: String,
    tables: Vec<SchemaTable>,
}

struct SshTunnel {
    local_port: u16,
    _listener_task: tokio::task::JoinHandle<()>,
}

impl Drop for SshTunnel {
    fn drop(&mut self) {
        self._listener_task.abort();
    }
}

struct SshHandler {
    host: String,
    port: u16,
}

fn ssh_known_hosts_path() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    PathBuf::from(&home).join(".ssh").join("known_hosts")
}

impl russh::client::Handler for SshHandler {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let host = self.host.clone();
        let port = self.port;
        let key = server_public_key.clone();
        async move {
            let known_hosts_path = ssh_known_hosts_path();
            match russh::keys::known_hosts::check_known_hosts_path(
                &host,
                port,
                &key,
                &known_hosts_path,
            ) {
                Ok(true) => Ok(true),
                Ok(false) => {
                    // Unknown host — TOFU: save and accept
                    let _ = russh::keys::known_hosts::learn_known_hosts_path(
                        &host,
                        port,
                        &key,
                        &known_hosts_path,
                    );
                    Ok(true)
                }
                Err(russh::keys::Error::KeyChanged { line }) => Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "SSH host key verification failed: the server key for {}:{} has CHANGED \
               (known_hosts line {}).\n\
               This could indicate a man-in-the-middle attack.\n\
               If you trust this change, remove line {} from {:?}.",
                        host, port, line, line, known_hosts_path
                    ),
                )
                .into()),
                Err(_) => {
                    // File doesn't exist or parse error — treat as unknown, TOFU
                    let _ = russh::keys::known_hosts::learn_known_hosts_path(
                        &host,
                        port,
                        &key,
                        &known_hosts_path,
                    );
                    Ok(true)
                }
            }
        }
    }
}

struct ConnectionCache {
    fingerprint: String,
    pool: Pool,
    _tunnel: Option<SshTunnel>,
}

struct RunningQueryEntry {
    connection_id: u32,
    pool: Pool,
}

static RUNNING_QUERIES: std::sync::LazyLock<Mutex<HashMap<String, RunningQueryEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

static SCHEMA_CACHE: std::sync::LazyLock<Mutex<HashMap<String, SchemaInfo>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// Window label constants — must match the labels in tauri.conf.json.
const WIN_MAIN: &str = "main";
const WIN_QUERY: &str = "query";
const WIN_SETTINGS: &str = "settings";

/// Tracks the label of the currently focused window.
/// `is_focused()` can be unreliable on Windows right after window creation,
/// so we explicitly track via `WindowEvent::Focused`.
struct ActiveWindow(Mutex<String>);

fn escape_identifier(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn connection_fingerprint(request: &ConnectionRequest) -> String {
    let ssh_part = match &request.ssh {
        Some(ssh) if ssh.enabled => match &ssh.config_host {
            Some(alias) if !alias.trim().is_empty() => format!("ssh-config:{alias}"),
            _ => format!(
                "ssh:{}:{}:{}:{}:{}",
                ssh.host,
                ssh.port,
                ssh.username,
                ssh.private_key_path.as_deref().unwrap_or(""),
                ssh.auth_method
            ),
        },
        _ => String::new(),
    };
    format!(
        "{}:{}:{}:ssl={}:ca={}|{}",
        request.mysql.host,
        request.mysql.port,
        request.mysql.username,
        request.mysql.ssl_mode,
        request.mysql.tls_ca_cert_path.as_deref().unwrap_or(""),
        ssh_part
    )
}

async fn get_or_create_pool_async(
    cache: &Arc<Mutex<Option<ConnectionCache>>>,
    request: &ConnectionRequest,
) -> Result<Pool, String> {
    let fp = connection_fingerprint(request);

    // Check cache (short lock, no await)
    {
        let guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
        if let Some(ref cached) = *guard {
            if cached.fingerprint == fp {
                return Ok(cached.pool.clone());
            }
        }
    }

    // Drop old cache first (frees SSH tunnel port)
    {
        let mut guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
        *guard = None;
    }

    // Set up SSH tunnel if needed (async)
    let (target_host, target_port, tunnel) = match &request.ssh {
        Some(ssh) if ssh.enabled => {
            let tunnel = start_ssh_tunnel(ssh, &request.mysql.host, request.mysql.port).await?;
            ("127.0.0.1".to_string(), tunnel.local_port, Some(tunnel))
        }
        _ => (request.mysql.host.clone(), request.mysql.port, None),
    };

    // Build MySQL pool (blocking)
    let mut mysql_no_db = request.mysql.clone();
    mysql_no_db.database = None;
    let pool = tauri::async_runtime::spawn_blocking(move || {
        let opts = build_opts(&mysql_no_db, &target_host, target_port);
        let pool = Pool::new(opts).map_err(|e| format!("Failed to build pool: {e}"))?;
        let _conn = pool
            .get_conn()
            .map_err(|e| format!("Failed to connect MySQL: {e}"))?;
        Ok::<Pool, String>(pool)
    })
    .await
    .map_err(|e| format!("Task error: {e}"))??;

    // Update cache
    {
        let mut guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
        *guard = Some(ConnectionCache {
            fingerprint: fp,
            pool: pool.clone(),
            _tunnel: tunnel,
        });
    }

    Ok(pool)
}

fn build_opts(mysql: &MySqlConfig, host: &str, port: u16) -> OptsBuilder {
    let mut builder = OptsBuilder::new();
    builder = builder
        .ip_or_hostname(Some(host.to_string()))
        .tcp_port(port)
        .user(Some(mysql.username.clone()))
        .pass(Some(mysql.password.clone()))
        .stmt_cache_size(Some(0));

    if let Some(db) = &mysql.database {
        if !db.trim().is_empty() {
            builder = builder.db_name(Some(db.clone()));
        }
    }

    match mysql.ssl_mode.as_str() {
        "REQUIRED" => {
            builder = builder.ssl_opts(Some(
                SslOpts::default()
                    .with_danger_accept_invalid_certs(true)
                    .with_danger_skip_domain_validation(true),
            ));
        }
        "VERIFY_CA" => {
            let mut ssl = SslOpts::default().with_danger_skip_domain_validation(true);
            if let Some(p) = &mysql.tls_ca_cert_path {
                if !p.trim().is_empty() {
                    ssl = ssl.with_root_cert_path(Some(PathBuf::from(p)));
                }
            }
            builder = builder.ssl_opts(Some(ssl));
        }
        "VERIFY_IDENTITY" => {
            let mut ssl = SslOpts::default();
            if let Some(p) = &mysql.tls_ca_cert_path {
                if !p.trim().is_empty() {
                    ssl = ssl.with_root_cert_path(Some(PathBuf::from(p)));
                }
            }
            builder = builder.ssl_opts(Some(ssl));
        }
        _ => { /* DISABLED or unknown — no ssl_opts */ }
    }

    builder
}

fn profiles_path(app: &AppHandle) -> Result<PathBuf, String> {
    let resolver = app.path();
    resolver
        .resolve("connections.json", tauri::path::BaseDirectory::AppConfig)
        .map_err(|e| format!("Failed to resolve config path: {e}"))
}

fn load_profiles(app: &AppHandle) -> Result<ConnectionProfileStore, String> {
    let path = profiles_path(app)?;
    if !path.exists() {
        return Ok(ConnectionProfileStore {
            version: 2,
            groups: Vec::new(),
            items: Vec::new(),
        });
    }
    let content =
        std::fs::read_to_string(&path).map_err(|e| format!("Failed to read profiles: {e}"))?;
    let mut store: ConnectionProfileStore =
        serde_json::from_str(&content).map_err(|e| format!("Failed to parse profiles: {e}"))?;

    // Migrate from version < 2: assign sequential order to items
    if store.version < 2 {
        for (i, item) in store.items.iter_mut().enumerate() {
            item.order = (i as u32 + 1) * 1000;
        }
        store.version = 2;
        save_profiles(app, &store)?;
    }

    // Migrate legacy tls_enabled/tls_skip_verify → ssl_mode
    let mut ssl_migrated = false;
    for item in store.items.iter_mut() {
        let m = &mut item.request.mysql;
        if m.tls_enabled {
            m.ssl_mode = if m.tls_skip_verify {
                "REQUIRED"
            } else {
                "VERIFY_IDENTITY"
            }
            .to_string();
            m.tls_enabled = false;
            m.tls_skip_verify = false;
            ssl_migrated = true;
        }
    }
    if ssl_migrated {
        save_profiles(app, &store)?;
    }

    // Migrate passwords from JSON to keyring
    let mut pw_migrated = false;
    for item in store.items.iter_mut() {
        let pw = &item.request.mysql.password;
        if !pw.is_empty() {
            let _ = set_password(&item.id, pw);
            item.request.mysql.password = String::new();
            pw_migrated = true;
        }
    }
    if pw_migrated {
        save_profiles(app, &store)?;
    }

    Ok(store)
}

fn save_profiles(app: &AppHandle, store: &ConnectionProfileStore) -> Result<(), String> {
    let path = profiles_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    let data = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed to serialize profiles: {e}"))?;
    std::fs::write(&path, data).map_err(|e| format!("Failed to write profiles: {e}"))?;
    // Mirror to the external sync file (if configured) so every profile change from
    // any window is pushed out automatically (#47). Passwords are keyring-only and are
    // already empty in the store, so no secret is written.
    mirror_to_sync(app, store);
    Ok(())
}

// ── External profile sync (path-based) (#47) ──

fn sync_path_file(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resolve("sync_path", tauri::path::BaseDirectory::AppConfig)
        .map_err(|e| format!("Failed to resolve sync-path config: {e}"))
}

fn read_sync_path(app: &AppHandle) -> String {
    match sync_path_file(app) {
        Ok(p) => std::fs::read_to_string(p)
            .unwrap_or_default()
            .trim()
            .to_string(),
        Err(_) => String::new(),
    }
}

// Fields holding a path into the local filesystem. These are meaningless (or wrong) on
// another machine — the same key/certificate lives under a different user directory, or
// not at all — so they are excluded from the sync file and never overwrite a local value
// on import (#80). `config_host` is deliberately NOT machine-local: it is an alias into
// the user's own ~/.ssh/config, which is typically kept identical across machines.
fn clear_machine_local_paths(profile: &mut ConnectionProfile) {
    profile.request.mysql.tls_ca_cert_path = None;
    if let Some(ssh) = profile.request.ssh.as_mut() {
        ssh.private_key_path = None;
    }
}

// Serialize the store for the external sync file with ALL secrets and machine-local
// paths stripped. Secrets live in the keyring and are `skip_serializing` besides, so
// clearing them here is defense-in-depth (it also scrubs legacy plaintext SSH secrets
// that predate the keyring migration); the path scrubbing, by contrast, is load-bearing.
fn sanitized_store_json(store: &ConnectionProfileStore) -> Result<String, serde_json::Error> {
    let mut clone = store.clone();
    for item in clone.items.iter_mut() {
        item.request.mysql.password = String::new();
        if let Some(ssh) = item.request.ssh.as_mut() {
            ssh.passphrase = String::new();
            ssh.ssh_password = String::new();
        }
        clear_machine_local_paths(item);
    }
    serde_json::to_string_pretty(&clone)
}

// Write the (secret-stripped) store to the sync file, creating parent dirs. Returns an
// error so callers that want a truthful result (manual export) can surface it.
fn write_sync_file(path: &str, store: &ConnectionProfileStore) -> Result<(), String> {
    let data =
        sanitized_store_json(store).map_err(|e| format!("Failed to serialize sync data: {e}"))?;
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create sync dir: {e}"))?;
        }
    }
    std::fs::write(path, data).map_err(|e| format!("Failed to write sync file: {e}"))
}

// Best-effort mirror on every save (the sync folder may be temporarily offline; a failure
// here must not fail the profile save).
fn mirror_to_sync(app: &AppHandle, store: &ConnectionProfileStore) {
    let path = read_sync_path(app);
    if path.is_empty() {
        return;
    }
    let _ = write_sync_file(&path, store);
}

#[tauri::command]
fn get_sync_path(app: AppHandle) -> String {
    read_sync_path(&app)
}

#[tauri::command]
fn set_sync_path(app: AppHandle, path: String) -> Result<(), String> {
    let file = sync_path_file(&app)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    std::fs::write(&file, path.trim()).map_err(|e| format!("Failed to write sync path: {e}"))?;
    Ok(())
}

// Drop anything the sync file has no business carrying: secrets (keyring-only) and the
// clear_* flags, which are one-shot UI commands to delete a keyring entry and would
// otherwise be injectable through a hand-edited sync file.
fn scrub_incoming_profile(profile: &mut ConnectionProfile) {
    profile.request.mysql.password = String::new();
    if let Some(ssh) = profile.request.ssh.as_mut() {
        ssh.passphrase = String::new();
        ssh.ssh_password = String::new();
    }
    profile.clear_password = false;
    profile.clear_ssh_passphrase = false;
    profile.clear_ssh_password = false;
    // A sync file written before this change still carries the origin machine's paths.
    // Dropping them here means a profile arriving for the first time shows an empty path
    // the user can fill in, rather than silently pointing at a key that does not exist.
    clear_machine_local_paths(profile);
}

// Machine-local paths never cross machines: whatever this machine has (including nothing)
// wins outright over the incoming value. Combined with the scrub above, a path from the
// sync file can never reach the local store.
fn carry_over_machine_local_paths(local: &ConnectionProfile, incoming: &mut ConnectionProfile) {
    incoming
        .request
        .mysql
        .tls_ca_cert_path
        .clone_from(&local.request.mysql.tls_ca_cert_path);
    if let Some(new_ssh) = incoming.request.ssh.as_mut() {
        new_ssh.private_key_path = local
            .request
            .ssh
            .as_ref()
            .and_then(|s| s.private_key_path.clone());
    }
}

// Pull the external sync file and merge it into the local store (id-based; the file
// wins for existing entries except machine-local paths, local-only entries are kept,
// deletions do not propagate).
// Runs on a blocking thread — the sync file may be a cloud on-demand placeholder that
// blocks for seconds while it hydrates, which must not stall the IPC thread.
#[tauri::command]
async fn sync_import(app: AppHandle) -> Result<ProfileListResponse, String> {
    tauri::async_runtime::spawn_blocking(move || sync_import_blocking(app))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

fn sync_import_blocking(app: AppHandle) -> Result<ProfileListResponse, String> {
    let path = read_sync_path(&app);
    // Resilient: a missing / unreadable / malformed sync file must never break the
    // profile list — fall back to the local store.
    if path.is_empty() {
        return list_profiles(app);
    }
    let incoming: ConnectionProfileStore = match std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
    {
        Some(store) => store,
        None => return list_profiles(app),
    };

    let mut store = load_profiles(&app)?;
    let before = serde_json::to_string(&store).unwrap_or_default();
    for g in incoming.groups {
        if let Some(existing) = store.groups.iter_mut().find(|x| x.id == g.id) {
            *existing = g;
        } else {
            store.groups.push(g);
        }
    }
    for mut it in incoming.items {
        // The sync file is externally writable (a shared cloud folder, possibly hand
        // edited), so never let it carry secrets or the transient clear_* commands into
        // the local store.
        scrub_incoming_profile(&mut it);
        if let Some(existing) = store.items.iter_mut().find(|x| x.id == it.id) {
            carry_over_machine_local_paths(existing, &mut it);
            *existing = it;
        } else {
            store.items.push(it);
        }
    }
    // Only persist (and re-mirror) when the merge actually changed something, so the
    // frequent startup/focus refresh doesn't churn disk or bounce the sync file.
    if serde_json::to_string(&store).unwrap_or_default() != before {
        save_profiles(&app, &store)?;
    }
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: None,
    })
}

// Manually push the current store to the external sync file (surfaces write errors).
#[tauri::command]
fn sync_export(app: AppHandle) -> Result<bool, String> {
    let path = read_sync_path(&app);
    if path.is_empty() {
        return Ok(false);
    }
    let store = load_profiles(&app)?;
    write_sync_file(&path, &store)?;
    Ok(true)
}

// Native save-dialog that returns the chosen path without writing (for choosing the
// sync file location, which may not exist yet).
#[tauri::command]
async fn pick_sync_path() -> Result<Option<String>, String> {
    let dialog = rfd::AsyncFileDialog::new()
        .set_file_name("musql-profiles-sync.json")
        .add_filter("JSON files", &["json"])
        .save_file()
        .await;
    Ok(dialog.map(|h| h.path().to_string_lossy().into_owned()))
}

fn generate_profile_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_millis(0))
        .as_millis();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    // Per-call random component (RandomState is OS-seeded per process) so two machines
    // creating their first id in the same millisecond don't collide and clobber each
    // other's profile on sync merge (#47).
    let mut h = RandomState::new().build_hasher();
    h.write_u64(seq);
    let rnd = h.finish() & 0xff_ffff;
    format!("p{ts}_{seq}_{rnd:06x}")
}

const KEYRING_SERVICE: &str = "musql";

// ── Generic keyring helpers ──

fn keyring_get(key: &str) -> String {
    let entry = match keyring::Entry::new(KEYRING_SERVICE, key) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };
    entry.get_password().unwrap_or_default()
}

fn keyring_set(key: &str, value: &str) -> Result<(), String> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, key).map_err(|e| format!("Keyring error: {e}"))?;
    if value.is_empty() {
        let _ = entry.delete_credential();
    } else {
        entry
            .set_password(value)
            .map_err(|e| format!("Keyring save error: {e}"))?;
    }
    Ok(())
}

fn keyring_delete(key: &str) {
    if let Ok(entry) = keyring::Entry::new(KEYRING_SERVICE, key) {
        let _ = entry.delete_credential();
    }
}

// ── MySQL password ──

fn get_password(profile_id: &str) -> String {
    keyring_get(profile_id)
}

fn set_password(profile_id: &str, password: &str) -> Result<(), String> {
    keyring_set(profile_id, password)
}

fn delete_password(profile_id: &str) {
    keyring_delete(profile_id);
}

#[tauri::command]
fn has_password(profile_id: String) -> bool {
    !get_password(&profile_id).is_empty()
}

// ── SSH passphrase ──

fn get_ssh_passphrase(profile_id: &str) -> String {
    keyring_get(&format!("{profile_id}:ssh_passphrase"))
}

fn set_ssh_passphrase(profile_id: &str, passphrase: &str) -> Result<(), String> {
    keyring_set(&format!("{profile_id}:ssh_passphrase"), passphrase)
}

fn delete_ssh_passphrase(profile_id: &str) {
    keyring_delete(&format!("{profile_id}:ssh_passphrase"));
}

#[tauri::command]
fn has_ssh_passphrase(profile_id: String) -> bool {
    !get_ssh_passphrase(&profile_id).is_empty()
}

// ── SSH password ──

fn get_ssh_password(profile_id: &str) -> String {
    keyring_get(&format!("{profile_id}:ssh_password"))
}

fn set_ssh_password(profile_id: &str, password: &str) -> Result<(), String> {
    keyring_set(&format!("{profile_id}:ssh_password"), password)
}

fn delete_ssh_password(profile_id: &str) {
    keyring_delete(&format!("{profile_id}:ssh_password"));
}

#[tauri::command]
fn has_ssh_password(profile_id: String) -> bool {
    !get_ssh_password(&profile_id).is_empty()
}

fn resolve_ssh_password(
    request: &mut ConnectionRequest,
    profile_id: Option<&str>,
) -> Result<(), String> {
    let Some(ssh) = request.ssh.as_mut() else {
        return Ok(());
    };
    // The settings form always submits an ssh block, so a disabled tunnel or the other
    // auth method can leave a stale 1Password reference behind. Resolving it would fetch
    // a secret nothing will use — and turn a failed fetch into a failed connection.
    if !ssh.enabled || ssh.auth_method != "password" {
        return Ok(());
    }
    if !ssh.ssh_password.is_empty() {
        return Ok(());
    }
    let Some(id) = profile_id.filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    ssh.ssh_password = get_ssh_password(id);
    if !ssh.ssh_password.is_empty() || !ssh.save_ssh_password {
        return Ok(());
    }
    if let Some(fetched) = onepassword::read_optional(ssh.op_password_ref.as_ref()) {
        let value = fetched.map_err(|e| format!("1Password (SSH password): {e}"))?;
        // Best-effort cache: we already hold the secret, so a keyring failure must not
        // fail the connection — it only means 1Password is consulted again next time.
        let _ = set_ssh_password(id, &value);
        ssh.ssh_password = value;
    }
    Ok(())
}

// ── 1Password ──

/// Whether the `op` CLI is installed. The UI hides the 1Password fields when it is not.
/// Async because the probe spawns a process, which must not run on the main thread.
#[tauri::command]
async fn op_available() -> bool {
    tauri::async_runtime::spawn_blocking(onepassword::is_available)
        .await
        .unwrap_or(false)
}

/// Resolve a secret reference on demand — the settings screen's "fetch from 1Password"
/// button. Blocking work (and a possible Windows Hello prompt) is kept off the IPC thread.
#[tauri::command]
async fn op_read_secret(reference: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || onepassword::read_secret(&reference))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

/// Items for the reference picker. Cached per session; `refresh` forces a re-fetch.
#[tauri::command]
async fn op_list_items(refresh: Option<bool>) -> Result<Vec<onepassword::OpItem>, String> {
    tauri::async_runtime::spawn_blocking(move || onepassword::list_items(refresh.unwrap_or(false)))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

/// Referenceable fields of one item. Secret values are dropped before this returns.
#[tauri::command]
async fn op_list_fields(item_id: String) -> Result<Vec<onepassword::OpField>, String> {
    tauri::async_runtime::spawn_blocking(move || onepassword::list_fields(&item_id))
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

// Resolve all three credentials off the async runtime: hitting 1Password shells out to
// the CLI, which can sit for many seconds while the user approves a Windows Hello prompt.
async fn resolve_credentials(
    request: &mut ConnectionRequest,
    profile_id: Option<&str>,
) -> Result<(), String> {
    let mut owned = request.clone();
    let id = profile_id.map(str::to_string);
    let resolved = tauri::async_runtime::spawn_blocking(move || {
        resolve_password(&mut owned, id.as_deref())?;
        resolve_ssh_passphrase(&mut owned, id.as_deref())?;
        resolve_ssh_password(&mut owned, id.as_deref())?;
        Ok::<ConnectionRequest, String>(owned)
    })
    .await
    .map_err(|e| format!("Task error: {e}"))??;
    *request = resolved;
    Ok(())
}

// ── AI keyring ──

fn ai_keyring_key(provider: &AiProvider) -> String {
    match provider {
        AiProvider::Claude => "ai:claude".to_string(),
        AiProvider::OpenAi => "ai:openai".to_string(),
        AiProvider::Gemini => "ai:gemini".to_string(),
    }
}

// AI keys follow the same rule as connection credentials: the keyring answers when it has
// the key, otherwise a configured 1Password reference is resolved once and written back.
// The reference lives in the UI's localStorage (AI settings are not part of the synced
// profile store), so it arrives as a command argument rather than from disk.
async fn resolve_ai_api_key(
    provider: &AiProvider,
    op_ref: Option<String>,
) -> Result<String, String> {
    let key = get_ai_api_key(provider);
    if !key.is_empty() {
        return Ok(key);
    }
    let Some(reference) = op_ref.filter(|s| !s.trim().is_empty()) else {
        return Err("AI API key not configured".to_string());
    };
    let provider = provider.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let value = onepassword::read_secret(&reference)
            .map_err(|e| format!("1Password (AI API key): {e}"))?;
        // Best-effort cache, as with the connection credentials.
        let _ = set_ai_api_key(&provider, &value);
        Ok(value)
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

fn get_ai_api_key(provider: &AiProvider) -> String {
    keyring_get(&ai_keyring_key(provider))
}

fn set_ai_api_key(provider: &AiProvider, api_key: &str) -> Result<(), String> {
    keyring_set(&ai_keyring_key(provider), api_key)
}

// ── Docker keyring ──

fn docker_keyring_key(container_id: &str) -> String {
    format!("docker:{container_id}")
}

#[tauri::command]
fn get_docker_password(container_id: String) -> String {
    keyring_get(&docker_keyring_key(&container_id))
}

#[tauri::command]
fn save_docker_password(container_id: String, password: String) -> Result<(), String> {
    keyring_set(&docker_keyring_key(&container_id), &password)
}

// ── Schema fetching ──

fn fetch_schema(pool: &Pool, database: &str) -> Result<SchemaInfo, String> {
    let mut conn = pool
        .get_conn()
        .map_err(|e| format!("Failed to get connection: {e}"))?;
    conn.query_drop(format!("USE {}", escape_identifier(database)))
        .map_err(|e| format!("Failed to switch database: {e}"))?;

    let rows: Vec<Row> = conn
        .exec(
            "SELECT TABLE_NAME, COLUMN_NAME, DATA_TYPE, COLUMN_KEY, IS_NULLABLE \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = ? \
             ORDER BY TABLE_NAME, ORDINAL_POSITION",
            (database,),
        )
        .map_err(|e| format!("Schema query failed: {e}"))?;

    let mut table_map: std::collections::BTreeMap<String, Vec<SchemaColumn>> =
        std::collections::BTreeMap::new();

    for row in rows {
        let table_name: String = row.get(0).unwrap_or_default();
        let col = SchemaColumn {
            name: row.get(1).unwrap_or_default(),
            data_type: row.get(2).unwrap_or_default(),
            column_key: row.get(3).unwrap_or_default(),
            is_nullable: row.get(4).unwrap_or_default(),
        };
        table_map.entry(table_name).or_default().push(col);
    }

    // Limit to 100 tables
    let tables: Vec<SchemaTable> = table_map
        .into_iter()
        .take(100)
        .map(|(name, columns)| SchemaTable { name, columns })
        .collect();

    Ok(SchemaInfo {
        database: database.to_string(),
        tables,
    })
}

// ── AI prompt building ──

fn build_ai_prompt(schema: &SchemaInfo, text_before: &str, text_after: &str) -> String {
    let mut schema_text = String::new();
    for table in &schema.tables {
        schema_text.push_str(&format!("-- {}\n", table.name));
        for col in &table.columns {
            let key_info = match col.column_key.as_str() {
                "PRI" => " PK",
                "MUL" => " FK",
                "UNI" => " UQ",
                _ => "",
            };
            schema_text.push_str(&format!(
                "--   {} {}{}\n",
                col.name, col.data_type, key_info
            ));
        }
    }
    // Truncate schema text if too long
    if schema_text.len() > 8000 {
        schema_text.truncate(8000);
        schema_text.push_str("\n-- (truncated)\n");
    }

    format!(
        "You are a MySQL query assistant. Given the database schema below and the SQL context, \
         return ONLY the SQL fragment to insert at [CURSOR]. No explanation, no markdown, no code fences.\n\n\
         Database: {}\n\n\
         Schema:\n{}\n\
         SQL context:\n{}\n[CURSOR]\n{}",
        schema.database, schema_text, text_before, text_after
    )
}

// ── AI API call ──

async fn call_ai_api(
    provider: &AiProvider,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))?;

    match provider {
        AiProvider::Claude => {
            // thinking を明示的に無効化: Sonnet 5 以降は未指定だと adaptive thinking が
            // 既定で有効になり、thinking トークンが max_tokens を消費して SQL が
            // 途切れるため(SQL 断片生成にはレイテンシ面でも thinking 不要)。
            let body = serde_json::json!({
                "model": model,
                "max_tokens": 2048,
                "thinking": {"type": "disabled"},
                "messages": [{"role": "user", "content": prompt}]
            });
            let resp = client
                .post("https://api.anthropic.com/v1/messages")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("Claude API request failed: {e}"))?;
            let status = resp.status();
            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Claude API response parse failed: {e}"))?;
            if !status.is_success() {
                let msg = json["error"]["message"].as_str().unwrap_or("Unknown error");
                return Err(format!("Claude API error ({status}): {msg}"));
            }
            json["content"][0]["text"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "Claude API: no text in response".to_string())
        }
        AiProvider::OpenAi => {
            let body = serde_json::json!({
                "model": model,
                "max_completion_tokens": 2048,
                "messages": [{"role": "user", "content": prompt}]
            });
            let resp = client
                .post("https://api.openai.com/v1/chat/completions")
                .header("Authorization", format!("Bearer {api_key}"))
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("OpenAI API request failed: {e}"))?;
            let status = resp.status();
            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("OpenAI API response parse failed: {e}"))?;
            if !status.is_success() {
                let msg = json["error"]["message"].as_str().unwrap_or("Unknown error");
                return Err(format!("OpenAI API error ({status}): {msg}"));
            }
            json["choices"][0]["message"]["content"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "OpenAI API: no content in response".to_string())
        }
        AiProvider::Gemini => {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
                model, api_key
            );
            let body = serde_json::json!({
                "contents": [{"parts": [{"text": prompt}]}]
            });
            let resp = client
                .post(&url)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| {
                    // Sanitise: reqwest errors may include the full URL containing the API key.
                    let msg = e.to_string();
                    if msg.contains("key=") {
                        "Gemini API request failed: network error".to_string()
                    } else {
                        format!("Gemini API request failed: {msg}")
                    }
                })?;
            let status = resp.status();
            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| format!("Gemini API response parse failed: {e}"))?;
            if !status.is_success() {
                let msg = json["error"]["message"].as_str().unwrap_or("Unknown error");
                return Err(format!("Gemini API error ({status}): {msg}"));
            }
            json["candidates"][0]["content"]["parts"][0]["text"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "Gemini API: no text in response".to_string())
        }
    }
}

// ── AI Tauri commands ──

#[tauri::command]
async fn ai_complete(
    text_before: String,
    text_after: String,
    provider: String,
    model: String,
    database: String,
    op_ref: Option<String>,
    state: tauri::State<'_, Arc<Mutex<Option<ConnectionCache>>>>,
) -> Result<String, String> {
    let ai_provider: AiProvider = serde_json::from_value(serde_json::json!(provider))
        .map_err(|_| format!("Invalid AI provider: {provider}"))?;

    let api_key = resolve_ai_api_key(&ai_provider, op_ref).await?;

    // Get pool + fingerprint from state
    let (pool, fingerprint) = {
        let guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
        match guard.as_ref() {
            Some(cached) => (cached.pool.clone(), cached.fingerprint.clone()),
            None => return Err("Not connected".to_string()),
        }
    };

    let cache_key = format!("{fingerprint}:{database}");

    // Check schema cache
    let schema = {
        let cache = SCHEMA_CACHE
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        cache.get(&cache_key).cloned()
    };

    let schema = match schema {
        Some(s) => s,
        None => {
            let db = database.clone();
            let ck = cache_key.clone();
            let schema = tauri::async_runtime::spawn_blocking(move || fetch_schema(&pool, &db))
                .await
                .map_err(|e| format!("Task error: {e}"))??;
            // Store in cache
            if let Ok(mut cache) = SCHEMA_CACHE.lock() {
                cache.insert(ck, schema.clone());
            }
            schema
        }
    };

    let prompt = build_ai_prompt(&schema, &text_before, &text_after);
    let result = call_ai_api(&ai_provider, &model, &api_key, &prompt).await?;
    Ok(result.trim().to_string())
}

fn build_ai_assist_prompt(
    schema: &SchemaInfo,
    prompt: &str,
    editor_content: &str,
    conversation_context: &str,
) -> String {
    let mut schema_text = String::new();
    for table in &schema.tables {
        schema_text.push_str(&format!("-- {}\n", table.name));
        for col in &table.columns {
            let key_info = match col.column_key.as_str() {
                "PRI" => " PK",
                "MUL" => " FK",
                "UNI" => " UQ",
                _ => "",
            };
            schema_text.push_str(&format!(
                "--   {} {}{}\n",
                col.name, col.data_type, key_info
            ));
        }
    }
    if schema_text.len() > 8000 {
        schema_text.truncate(8000);
        schema_text.push_str("\n-- (truncated)\n");
    }

    let mut parts = format!(
        "You are a MySQL query assistant. Given the database schema below, \
         write SQL based on the user's request. Return ONLY the SQL query. \
         No explanation, no markdown, no code fences.\n\n\
         Database: {}\n\n\
         Schema:\n{}",
        schema.database, schema_text
    );

    if !editor_content.trim().is_empty() {
        parts.push_str(&format!("\nCurrent SQL in editor:\n{}\n", editor_content));
    }
    if !conversation_context.trim().is_empty() {
        parts.push_str(&format!(
            "\nPrevious conversation:\n{}\n",
            conversation_context
        ));
    }
    parts.push_str(&format!("\nUser request: {}", prompt));
    parts
}

// Tauri commands take their arguments flat off the IPC payload, so grouping them into a
// struct would only move the noise to the call site in JS.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn ai_assist(
    prompt: String,
    editor_content: String,
    conversation_context: String,
    provider: String,
    model: String,
    database: String,
    op_ref: Option<String>,
    state: tauri::State<'_, Arc<Mutex<Option<ConnectionCache>>>>,
) -> Result<String, String> {
    let ai_provider: AiProvider = serde_json::from_value(serde_json::json!(provider))
        .map_err(|_| format!("Invalid AI provider: {provider}"))?;

    let api_key = resolve_ai_api_key(&ai_provider, op_ref).await?;

    let (pool, fingerprint) = {
        let guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
        match guard.as_ref() {
            Some(cached) => (cached.pool.clone(), cached.fingerprint.clone()),
            None => return Err("Not connected".to_string()),
        }
    };

    let cache_key = format!("{fingerprint}:{database}");

    let schema = {
        let cache = SCHEMA_CACHE
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        cache.get(&cache_key).cloned()
    };

    let schema = match schema {
        Some(s) => s,
        None => {
            let db = database.clone();
            let ck = cache_key.clone();
            let schema = tauri::async_runtime::spawn_blocking(move || fetch_schema(&pool, &db))
                .await
                .map_err(|e| format!("Task error: {e}"))??;
            if let Ok(mut cache) = SCHEMA_CACHE.lock() {
                cache.insert(ck, schema.clone());
            }
            schema
        }
    };

    let ai_prompt =
        build_ai_assist_prompt(&schema, &prompt, &editor_content, &conversation_context);
    let result = call_ai_api(&ai_provider, &model, &api_key, &ai_prompt).await?;

    // Strip code fences if present
    let trimmed = result.trim();
    let cleaned = if trimmed.starts_with("```") {
        let inner = trimmed
            .trim_start_matches("```sql")
            .trim_start_matches("```SQL")
            .trim_start_matches("```")
            .trim_end_matches("```");
        inner.trim().to_string()
    } else {
        trimmed.to_string()
    };

    Ok(cleaned)
}

#[tauri::command]
fn save_ai_api_key(provider: String, api_key: String) -> Result<(), String> {
    let ai_provider: AiProvider =
        serde_json::from_value(serde_json::json!(provider)).map_err(|_| "Invalid AI provider")?;
    set_ai_api_key(&ai_provider, &api_key)
}

#[tauri::command]
fn has_ai_api_key(provider: String) -> bool {
    let ai_provider: AiProvider = match serde_json::from_value(serde_json::json!(provider)) {
        Ok(p) => p,
        Err(_) => return false,
    };
    !get_ai_api_key(&ai_provider).is_empty()
}

#[tauri::command]
fn clear_schema_cache() -> Result<(), String> {
    let mut cache = SCHEMA_CACHE
        .lock()
        .map_err(|e| format!("Lock error: {e}"))?;
    cache.clear();
    Ok(())
}

fn parse_ssh_config_hosts(content: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("Host ")
            .or_else(|| trimmed.strip_prefix("Host\t"))
            .or_else(|| trimmed.strip_prefix("host "))
            .or_else(|| trimmed.strip_prefix("host\t"))
        {
            for pattern in rest.split_whitespace() {
                if !pattern.contains('*') && !pattern.contains('?') {
                    hosts.push(pattern.to_string());
                }
            }
        }
    }
    hosts
}

#[tauri::command]
fn list_ssh_config_hosts() -> Vec<String> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    if home.is_empty() {
        return vec![];
    }
    let path = std::path::Path::new(&home).join(".ssh").join("config");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    parse_ssh_config_hosts(&content)
}

#[derive(serde::Serialize)]
struct SshConfigResolved {
    host: String,
    port: u16,
    user: Option<String>,
    identity_file: Option<String>,
}

#[tauri::command]
fn resolve_ssh_config(alias: &str) -> SshConfigResolved {
    let (host, port, user, identity_file) = resolve_ssh_config_host(alias);
    SshConfigResolved {
        host,
        port,
        user,
        identity_file,
    }
}

fn parse_ssh_config_host(
    content: &str,
    alias: &str,
    home: &str,
) -> (String, u16, Option<String>, Option<String>) {
    let mut hostname: Option<String> = None;
    let mut port: Option<u16> = None;
    let mut user: Option<String> = None;
    let mut identity_file: Option<String> = None;
    let mut in_matching_block = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(rest) = trimmed
            .strip_prefix("Host ")
            .or_else(|| trimmed.strip_prefix("Host\t"))
            .or_else(|| trimmed.strip_prefix("host "))
            .or_else(|| trimmed.strip_prefix("host\t"))
        {
            in_matching_block = rest.split_whitespace().any(|p| p == alias);
            continue;
        }

        if !in_matching_block {
            continue;
        }

        let lower = trimmed.to_lowercase();
        let value = trimmed
            .split_once(char::is_whitespace)
            .map(|x| x.1)
            .unwrap_or("")
            .trim();

        if lower.starts_with("hostname") && hostname.is_none() {
            hostname = Some(value.to_string());
        } else if lower.starts_with("port") && port.is_none() {
            port = value.parse().ok();
        } else if lower.starts_with("user")
            && !lower.starts_with("userknownhostsfile")
            && user.is_none()
        {
            user = Some(value.to_string());
        } else if lower.starts_with("identityfile") && identity_file.is_none() {
            let expanded = if value.starts_with("~/") || value == "~" {
                value.replacen("~", home, 1)
            } else {
                value.to_string()
            };
            identity_file = Some(expanded);
        }
    }

    (
        hostname.unwrap_or_else(|| alias.to_string()),
        port.unwrap_or(22),
        user,
        identity_file,
    )
}

fn resolve_ssh_config_host(alias: &str) -> (String, u16, Option<String>, Option<String>) {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    if home.is_empty() {
        return (alias.to_string(), 22, None, None);
    }
    let path = std::path::Path::new(&home).join(".ssh").join("config");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (alias.to_string(), 22, None, None),
    };
    parse_ssh_config_host(&content, alias, &home)
}

async fn authenticate_ssh(
    session: &mut russh::client::Handle<SshHandler>,
    username: &str,
    identity_file: Option<&str>,
    passphrase: Option<&str>,
    auth_method: &str,
    ssh_password: Option<&str>,
) -> Result<bool, String> {
    // Password authentication
    if auth_method == "password" {
        if let Some(pw) = ssh_password.filter(|s| !s.is_empty()) {
            match session.authenticate_password(username, pw).await {
                Ok(res) if res.success() => return Ok(true),
                Ok(_) => return Err("SSH password authentication rejected.".into()),
                Err(e) => return Err(format!("SSH password auth failed: {e}")),
            }
        }
        return Err("SSH password is required.".into());
    }

    // Key-based authentication
    let key_path = identity_file.map(|s| s.trim()).filter(|s| !s.is_empty());

    // 1. Try explicit private key file (skip .pub files)
    if let Some(path) = key_path {
        if !path.ends_with(".pub") {
            if let Ok(key) = russh::keys::load_secret_key(path, passphrase) {
                let hash_alg = if key.algorithm().is_rsa() {
                    Some(russh::keys::HashAlg::Sha256)
                } else {
                    None
                };
                match session
                    .authenticate_publickey(
                        username,
                        russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                    )
                    .await
                {
                    Ok(res) if res.success() => return Ok(true),
                    _ => {} // auth rejected → fall through to agent
                }
            }
            // load failed (wrong passphrase / no passphrase given?) → fall through to agent
        }
    }

    // 2. Try SSH agent (with .pub hint for 1Password key selection)
    let pub_key_hint = key_path.and_then(|path| {
        let pub_path = if path.ends_with(".pub") {
            path.to_string()
        } else {
            format!("{path}.pub")
        };
        let content = std::fs::read_to_string(&pub_path).ok()?;
        russh::keys::PublicKey::from_openssh(content.trim()).ok()
    });

    #[cfg(windows)]
    {
        if let Ok(pipe) = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(r"\\.\pipe\openssh-ssh-agent")
        {
            let mut agent = russh::keys::agent::client::AgentClient::connect(pipe);
            if let Ok(identities) = agent.request_identities().await {
                let rsa_hash_alg = |key: &russh::keys::PublicKey| -> Option<russh::keys::HashAlg> {
                    if key.algorithm().is_rsa() {
                        Some(russh::keys::HashAlg::Sha256)
                    } else {
                        None
                    }
                };

                // If hint provided, try matching identity first
                if let Some(ref hint) = pub_key_hint {
                    for id in &identities {
                        let pk = id.public_key();
                        if pk.key_data() == hint.key_data() {
                            let hash_alg = rsa_hash_alg(&pk);
                            if let Ok(res) = session
                                .authenticate_publickey_with(
                                    username,
                                    pk.into_owned(),
                                    hash_alg,
                                    &mut agent,
                                )
                                .await
                            {
                                if res.success() {
                                    return Ok(true);
                                }
                            }
                        }
                    }
                }
                // Try all identities
                for id in &identities {
                    let pk = id.public_key();
                    let hash_alg = rsa_hash_alg(&pk);
                    if let Ok(res) = session
                        .authenticate_publickey_with(
                            username,
                            pk.into_owned(),
                            hash_alg,
                            &mut agent,
                        )
                        .await
                    {
                        if res.success() {
                            return Ok(true);
                        }
                    }
                }
            }
        }
    }

    #[cfg(not(windows))]
    {
        if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
            if let Ok(stream) = tokio::net::UnixStream::connect(&sock).await {
                let mut agent = russh::keys::agent::client::AgentClient::connect(stream);
                if let Ok(identities) = agent.request_identities().await {
                    if let Some(ref hint) = pub_key_hint {
                        for id in &identities {
                            let pk = id.public_key();
                            if pk.key_data() == hint.key_data() {
                                let hash_alg = if pk.algorithm().is_rsa() {
                                    Some(russh::keys::HashAlg::Sha256)
                                } else {
                                    None
                                };
                                if let Ok(res) = session
                                    .authenticate_publickey_with(
                                        username,
                                        pk.into_owned(),
                                        hash_alg,
                                        &mut agent,
                                    )
                                    .await
                                {
                                    if res.success() {
                                        return Ok(true);
                                    }
                                }
                            }
                        }
                    }
                    for id in &identities {
                        let pk = id.public_key();
                        let hash_alg = if pk.algorithm().is_rsa() {
                            Some(russh::keys::HashAlg::Sha256)
                        } else {
                            None
                        };
                        if let Ok(res) = session
                            .authenticate_publickey_with(
                                username,
                                pk.into_owned(),
                                hash_alg,
                                &mut agent,
                            )
                            .await
                        {
                            if res.success() {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Try default key files (only when no specific key was given)
    if key_path.is_none() {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();

        if !home.is_empty() {
            let ssh_dir = std::path::Path::new(&home).join(".ssh");
            for name in &["id_ed25519", "id_rsa", "id_ecdsa"] {
                let kp = ssh_dir.join(name);
                if kp.exists() {
                    if let Ok(key) = russh::keys::load_secret_key(&kp, None) {
                        let hash_alg = if key.algorithm().is_rsa() {
                            Some(russh::keys::HashAlg::Sha256)
                        } else {
                            None
                        };
                        if let Ok(res) = session
                            .authenticate_publickey(
                                username,
                                russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg),
                            )
                            .await
                        {
                            if res.success() {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }
    }

    Err(
    "SSH authentication failed: no valid key found. Specify a private key path in SSH settings."
      .to_string(),
  )
}

async fn start_ssh_tunnel(
    ssh: &SshConfig,
    mysql_host: &str,
    mysql_port: u16,
) -> Result<SshTunnel, String> {
    // Resolve SSH config host if specified
    let (host, port, config_user, config_identity) = match &ssh.config_host {
        Some(alias) if !alias.trim().is_empty() => resolve_ssh_config_host(alias),
        _ => (
            ssh.host.clone(),
            ssh.port,
            Some(ssh.username.clone()),
            ssh.private_key_path.clone(),
        ),
    };
    let username = config_user.unwrap_or_else(|| ssh.username.clone());
    let identity_file = config_identity.or_else(|| ssh.private_key_path.clone());

    // Connect via russh (with timeout)
    let config = Arc::new(russh::client::Config::default());
    let mut session = tokio::time::timeout(
        Duration::from_secs(8),
        russh::client::connect(
            config,
            (host.as_str(), port),
            SshHandler {
                host: host.clone(),
                port,
            },
        ),
    )
    .await
    .map_err(|_| format!("SSH tunnel timed out (connect to {host}:{port})"))?
    .map_err(|e| format!("SSH connection failed ({host}:{port}): {e}"))?;

    // Authenticate
    let passphrase = if ssh.passphrase.is_empty() {
        None
    } else {
        Some(ssh.passphrase.as_str())
    };
    let ssh_password = if ssh.ssh_password.is_empty() {
        None
    } else {
        Some(ssh.ssh_password.as_str())
    };
    let authenticated = authenticate_ssh(
        &mut session,
        &username,
        identity_file.as_deref(),
        passphrase,
        &ssh.auth_method,
        ssh_password,
    )
    .await?;
    if !authenticated {
        return Err("SSH authentication failed".to_string());
    }

    // Create local TCP listener on a free port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind local port: {e}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("Failed to read local addr: {e}"))?
        .port();

    // Spawn forwarding task
    let mysql_host = mysql_host.to_string();
    let task = tokio::spawn(async move {
        loop {
            let (mut tcp_stream, _) = match listener.accept().await {
                Ok(c) => c,
                Err(_) => break,
            };
            let channel = match session
                .channel_open_direct_tcpip(mysql_host.clone(), mysql_port as u32, "127.0.0.1", 0u32)
                .await
            {
                Ok(ch) => ch,
                Err(_) => continue,
            };
            let mut ssh_stream = channel.into_stream();
            tokio::spawn(async move {
                let _ = tokio::io::copy_bidirectional(&mut tcp_stream, &mut ssh_stream).await;
            });
        }
    });

    Ok(SshTunnel {
        local_port,
        _listener_task: task,
    })
}

fn mysql_value_to_json(value: Value) -> serde_json::Value {
    match value {
        Value::NULL => serde_json::Value::Null,
        Value::Bytes(v) => match String::from_utf8(v.clone()) {
            Ok(s) => json!(s),
            Err(_) => json!(format!("0x{}", hex::encode(v))),
        },
        Value::Int(v) => json!(v),
        Value::UInt(v) => json!(v),
        Value::Float(v) => json!(v),
        Value::Double(v) => json!(v),
        Value::Date(y, m, d, hh, mm, ss, micros) => {
            json!(format!(
                "{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}.{:06}",
                micros
            ))
        }
        Value::Time(is_neg, d, h, m, s, micros) => {
            let sign = if is_neg { "-" } else { "" };
            json!(format!("{sign}{d} {h:02}:{m:02}:{s:02}.{:06}", micros))
        }
    }
}

fn row_to_json(row: Row) -> Vec<serde_json::Value> {
    row.unwrap().into_iter().map(mysql_value_to_json).collect()
}

// Credentials resolve in a fixed order: a value already on the request (typed into the
// per-connection prompt) wins, then the keyring, then 1Password. A value fetched from
// 1Password is written straight back to the keyring, so the CLI is consulted once per
// machine rather than once per connection (#80).
fn resolve_password(
    request: &mut ConnectionRequest,
    profile_id: Option<&str>,
) -> Result<(), String> {
    if !request.mysql.password.is_empty() {
        return Ok(());
    }
    let Some(id) = profile_id.filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    request.mysql.password = get_password(id);
    // With "save to keyring" off there is nowhere to cache the result, so 1Password would
    // be consulted on every single connection — exactly the per-connection biometric
    // prompt this design avoids. That mode uses the per-connection prompt instead.
    if !request.mysql.password.is_empty() || !request.mysql.save_password {
        return Ok(());
    }
    if let Some(fetched) = onepassword::read_optional(request.mysql.op_ref.as_ref()) {
        let value = fetched.map_err(|e| format!("1Password (MySQL password): {e}"))?;
        // Best-effort cache: we already hold the secret, so a keyring failure must not
        // fail the connection — it only means 1Password is consulted again next time.
        let _ = set_password(id, &value);
        request.mysql.password = value;
    }
    Ok(())
}

fn resolve_ssh_passphrase(
    request: &mut ConnectionRequest,
    profile_id: Option<&str>,
) -> Result<(), String> {
    let Some(ssh) = request.ssh.as_mut() else {
        return Ok(());
    };
    // See resolve_ssh_password: skip when this secret cannot be in play.
    if !ssh.enabled || ssh.auth_method != "key" {
        return Ok(());
    }
    if !ssh.passphrase.is_empty() {
        return Ok(());
    }
    let Some(id) = profile_id.filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    ssh.passphrase = get_ssh_passphrase(id);
    if !ssh.passphrase.is_empty() || !ssh.save_ssh_passphrase {
        return Ok(());
    }
    if let Some(fetched) = onepassword::read_optional(ssh.op_passphrase_ref.as_ref()) {
        let value = fetched.map_err(|e| format!("1Password (SSH passphrase): {e}"))?;
        let _ = set_ssh_passphrase(id, &value);
        ssh.passphrase = value;
    }
    Ok(())
}

#[tauri::command]
async fn test_connection(
    mut request: ConnectionRequest,
    profile_id: Option<String>,
) -> Result<String, String> {
    resolve_credentials(&mut request, profile_id.as_deref()).await?;

    // Set up SSH tunnel if needed (async)
    let (target_host, target_port, _tunnel) = match &request.ssh {
        Some(ssh) if ssh.enabled => {
            let tunnel = start_ssh_tunnel(ssh, &request.mysql.host, request.mysql.port).await?;
            ("127.0.0.1".to_string(), tunnel.local_port, Some(tunnel))
        }
        _ => (request.mysql.host.clone(), request.mysql.port, None),
    };

    // MySQL connection test (blocking)
    let mysql = request.mysql.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let opts = build_opts(&mysql, &target_host, target_port);
        let pool = Pool::new(opts).map_err(|e| format!("Failed to build pool: {e}"))?;
        let mut conn = pool
            .get_conn()
            .map_err(|e| format!("Failed to connect MySQL: {e}"))?;
        conn.query_drop("SELECT 1")
            .map_err(|e| format!("MySQL ping failed: {e}"))?;
        Ok("Connection succeeded.".to_string())
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

#[tauri::command]
async fn run_query(
    mut request: ConnectionRequest,
    query: String,
    max_rows: Option<usize>,
    profile_id: Option<String>,
    tab_id: Option<String>,
    state: tauri::State<'_, Arc<Mutex<Option<ConnectionCache>>>>,
) -> Result<QueryResult, String> {
    if query.trim().is_empty() {
        return Err("Query is empty".to_string());
    }

    resolve_credentials(&mut request, profile_id.as_deref()).await?;
    let limit = max_rows.unwrap_or(500);

    // Get or create pool (async — SSH tunnel creation is async)
    let cache = Arc::clone(&state);
    let pool = get_or_create_pool_async(&cache, &request).await?;

    let db = request.mysql.database.clone();
    let tab_key = tab_id.unwrap_or_else(|| "__internal__".to_string());
    let tab_key_cleanup = tab_key.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = pool
            .get_conn()
            .map_err(|e| format!("Failed to get connection: {e}"))?;

        // Store connection ID + pool for cancel support (per tab)
        let cid = conn.connection_id();
        if let Ok(mut map) = RUNNING_QUERIES.lock() {
            map.insert(
                tab_key,
                RunningQueryEntry {
                    connection_id: cid,
                    pool: pool.clone(),
                },
            );
        }

        let result = (|| -> Result<QueryResult, String> {
            // Switch database if specified
            if let Some(ref db) = db {
                if !db.trim().is_empty() {
                    conn.query_drop(format!("USE {}", escape_identifier(db)))
                        .map_err(|e| format!("Failed to switch database: {e}"))?;
                }
            }

            let mut qr = conn
                .query_iter(&query)
                .map_err(|e| format!("Query failed: {e}"))?;

            let columns = qr
                .columns()
                .as_ref()
                .iter()
                .map(|c| c.name_str().to_string())
                .collect::<Vec<_>>();

            let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
            for next_row in qr.by_ref() {
                let row = next_row.map_err(|e| format!("Row read failed: {e}"))?;
                rows.push(row_to_json(row));
                if limit > 0 && rows.len() >= limit {
                    break;
                }
            }

            Ok(QueryResult {
                columns,
                rows,
                affected_rows: qr.affected_rows(),
            })
        })();

        if let Ok(mut map) = RUNNING_QUERIES.lock() {
            map.remove(&tab_key_cleanup);
        }
        result
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

#[tauri::command]
async fn cancel_query(tab_id: Option<String>) -> Result<(), String> {
    let tab_key = tab_id.unwrap_or_else(|| "__internal__".to_string());
    let entry = {
        let map = RUNNING_QUERIES
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        map.get(&tab_key).map(|e| (e.connection_id, e.pool.clone()))
    };
    let Some((cid, pool)) = entry else {
        return Ok(());
    };
    if cid == 0 {
        return Ok(());
    }

    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = pool
            .get_conn()
            .map_err(|e| format!("Failed to get cancel connection: {e}"))?;
        conn.query_drop(format!("KILL QUERY {cid}"))
            .map_err(|e| format!("Failed to cancel query: {e}"))
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

#[tauri::command]
fn disconnect_pool(
    state: tauri::State<'_, Arc<Mutex<Option<ConnectionCache>>>>,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
    *guard = None;
    Ok(())
}

// Encode export content to the requested charset. Unmappable characters are
// replaced with numeric character references by encoding_rs (never fails).
fn encode_export_content(content: &str, encoding: Option<&str>) -> Vec<u8> {
    match encoding.unwrap_or("utf-8") {
        "utf-8-bom" => {
            let mut v = Vec::with_capacity(content.len() + 3);
            v.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
            v.extend_from_slice(content.as_bytes());
            v
        }
        "shift_jis" => encoding_rs::SHIFT_JIS.encode(content).0.into_owned(),
        "euc-jp" => encoding_rs::EUC_JP.encode(content).0.into_owned(),
        _ => content.as_bytes().to_vec(),
    }
}

#[tauri::command]
async fn export_file(
    content: String,
    default_name: String,
    filter_name: String,
    extensions: Vec<String>,
    encoding: Option<String>,
) -> Result<bool, String> {
    let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
    let dialog = rfd::AsyncFileDialog::new()
        .set_file_name(&default_name)
        .add_filter(&filter_name, &ext_refs)
        .save_file()
        .await;

    match dialog {
        Some(handle) => {
            let bytes = encode_export_content(&content, encoding.as_deref());
            std::fs::write(handle.path(), &bytes)
                .map_err(|e| format!("Failed to write file: {e}"))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[tauri::command]
async fn pick_file(
    title: Option<String>,
    filter_name: Option<String>,
    extensions: Option<Vec<String>>,
) -> Result<Option<String>, String> {
    let mut dialog = rfd::AsyncFileDialog::new();
    if let Some(t) = &title {
        dialog = dialog.set_title(t);
    }
    if let Some(fname) = &filter_name {
        let exts = extensions.as_deref().unwrap_or(&[]);
        let ext_refs: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
        if !ext_refs.is_empty() {
            dialog = dialog.add_filter(fname, &ext_refs);
        }
    }
    let result = dialog.pick_file().await;
    Ok(result.map(|h| h.path().to_string_lossy().into_owned()))
}

#[tauri::command]
async fn export_profiles(app: AppHandle, include_passwords: bool) -> Result<bool, String> {
    let store = load_profiles(&app)?;
    let passwords = if include_passwords {
        let mut map = std::collections::HashMap::new();
        for item in &store.items {
            let pw = get_password(&item.id);
            if !pw.is_empty() {
                map.insert(item.id.clone(), pw);
            }
        }
        if map.is_empty() {
            None
        } else {
            Some(map)
        }
    } else {
        None
    };

    let ssh_passphrases = if include_passwords {
        let mut map = std::collections::HashMap::new();
        for item in &store.items {
            let pp = get_ssh_passphrase(&item.id);
            if !pp.is_empty() {
                map.insert(item.id.clone(), pp);
            }
        }
        if map.is_empty() {
            None
        } else {
            Some(map)
        }
    } else {
        None
    };

    let export = ExportData {
        version: store.version,
        groups: store.groups,
        items: store.items,
        passwords,
        ssh_passphrases,
    };

    let json = serde_json::to_string_pretty(&export)
        .map_err(|e| format!("Failed to serialize export data: {e}"))?;

    let dialog = rfd::AsyncFileDialog::new()
        .set_file_name("musql-profiles.json")
        .add_filter("JSON files", &["json"])
        .save_file()
        .await;

    match dialog {
        Some(handle) => {
            std::fs::write(handle.path(), json.as_bytes())
                .map_err(|e| format!("Failed to write file: {e}"))?;
            Ok(true)
        }
        None => Ok(false),
    }
}

#[tauri::command]
async fn import_profiles(
    app: AppHandle,
    mode: Option<String>,
    file_path: Option<String>,
) -> Result<Option<ImportResult>, String> {
    // 1. Read file
    let (import_path, import_data) = if let Some(ref path) = file_path {
        // Second call: read from the specified path
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {e}"))?;
        let data: ExportData = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse import file: {e}"))?;
        (path.clone(), data)
    } else {
        // First call: open file picker
        let dialog = rfd::AsyncFileDialog::new()
            .add_filter("JSON files", &["json"])
            .pick_file()
            .await;
        let handle = match dialog {
            Some(h) => h,
            None => return Ok(None),
        };
        let path_str = handle.path().to_string_lossy().into_owned();
        let content = std::fs::read_to_string(handle.path())
            .map_err(|e| format!("Failed to read file: {e}"))?;
        let data: ExportData = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse import file: {e}"))?;
        (path_str, data)
    };

    let store = load_profiles(&app)?;

    // 2. Conflict detection (first call only, mode == None)
    if mode.is_none() {
        let dup_groups: Vec<String> = import_data
            .groups
            .iter()
            .filter(|ig| store.groups.iter().any(|sg| sg.name == ig.name))
            .map(|ig| ig.name.clone())
            .collect();

        let dup_profiles: Vec<String> = import_data
            .items
            .iter()
            .filter(|ii| {
                // Match: same name AND same group (by group name mapping)
                let import_group_name = ii
                    .group_id
                    .as_ref()
                    .and_then(|gid| import_data.groups.iter().find(|g| g.id == *gid))
                    .map(|g| g.name.as_str());
                store.items.iter().any(|si| {
                    if si.name != ii.name {
                        return false;
                    }
                    let store_group_name = si
                        .group_id
                        .as_ref()
                        .and_then(|gid| store.groups.iter().find(|g| g.id == *gid))
                        .map(|g| g.name.as_str());
                    import_group_name == store_group_name
                })
            })
            .map(|ii| ii.name.clone())
            .collect();

        if !dup_groups.is_empty() || !dup_profiles.is_empty() {
            // Return conflicts without importing
            return Ok(Some(ImportResult {
                groups: store.groups,
                items: store.items,
                imported_count: 0,
                conflicts: Some(ImportConflicts {
                    groups: dup_groups,
                    profiles: dup_profiles,
                }),
                file_path: Some(import_path),
            }));
        }
        // No conflicts — fall through to "add" mode
    }

    let effective_mode = mode.as_deref().unwrap_or("add");

    // 3. Execute import
    let mut store = store; // make mutable
    let mut group_id_map = std::collections::HashMap::new();
    let max_group_order = store.groups.iter().map(|g| g.order).max().unwrap_or(0);
    let max_item_order = store.items.iter().map(|it| it.order).max().unwrap_or(0);
    let mut next_order = max_group_order.max(max_item_order) + 1000;

    for group in &import_data.groups {
        if effective_mode == "overwrite" {
            // Reuse existing group with same name
            if let Some(existing) = store.groups.iter().find(|sg| sg.name == group.name) {
                group_id_map.insert(group.id.clone(), existing.id.clone());
                continue;
            }
        }
        // "add" or no match in overwrite → create new
        let new_id = generate_profile_id();
        group_id_map.insert(group.id.clone(), new_id.clone());
        store.groups.push(ProfileGroup {
            id: new_id,
            name: group.name.clone(),
            order: next_order,
        });
        next_order += 1000;
    }

    let mut profile_id_map = std::collections::HashMap::new();
    let mut imported_count = 0usize;

    for item in &import_data.items {
        let mapped_group_id = item
            .group_id
            .as_ref()
            .and_then(|gid| group_id_map.get(gid).cloned());

        if effective_mode == "overwrite" {
            // Find existing profile with same name in the same (mapped) group
            if let Some(existing) = store
                .items
                .iter_mut()
                .find(|si| si.name == item.name && si.group_id == mapped_group_id)
            {
                // Overwrite: update fields, keep existing ID
                profile_id_map.insert(item.id.clone(), existing.id.clone());
                existing.request = item.request.clone();
                existing.color = item.color.clone();
                existing.tags = item.tags.clone();
                imported_count += 1;
                continue;
            }
        }

        // "add" or no match in overwrite → create new
        let new_id = generate_profile_id();
        profile_id_map.insert(item.id.clone(), new_id.clone());
        store.items.push(ConnectionProfile {
            id: new_id,
            name: item.name.clone(),
            group_id: mapped_group_id,
            order: next_order,
            color: item.color.clone(),
            tags: item.tags.clone(),
            request: item.request.clone(),
            clear_password: false,
            clear_ssh_passphrase: false,
            clear_ssh_password: false,
        });
        next_order += 1000;
        imported_count += 1;
    }

    save_profiles(&app, &store)?;

    // Import passwords
    if let Some(passwords) = &import_data.passwords {
        for (old_id, pw) in passwords {
            if let Some(new_id) = profile_id_map.get(old_id) {
                let _ = set_password(new_id, pw);
            }
        }
    }

    // Import SSH passphrases
    if let Some(ssh_passphrases) = &import_data.ssh_passphrases {
        for (old_id, pp) in ssh_passphrases {
            if let Some(new_id) = profile_id_map.get(old_id) {
                let _ = set_ssh_passphrase(new_id, pp);
            }
        }
    }

    Ok(Some(ImportResult {
        groups: store.groups,
        items: store.items,
        imported_count,
        conflicts: None,
        file_path: None,
    }))
}

#[tauri::command]
fn list_profiles(app: AppHandle) -> Result<ProfileListResponse, String> {
    let store = load_profiles(&app)?;
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: None,
    })
}

#[tauri::command]
fn save_profile(
    app: AppHandle,
    mut profile: ConnectionProfile,
) -> Result<ProfileListResponse, String> {
    if profile.name.trim().is_empty() {
        return Err("Profile name is empty".to_string());
    }
    if profile.id.trim().is_empty() {
        profile.id = generate_profile_id();
        // Assign order: max order in same group + 1000
        let store_tmp = load_profiles(&app)?;
        let max_order = store_tmp
            .items
            .iter()
            .filter(|it| it.group_id == profile.group_id)
            .map(|it| it.order)
            .max()
            .unwrap_or(0);
        profile.order = max_order + 1000;
    }
    // Extract password: clear flag → delete; non-empty → save (if save_password); empty → keep existing
    let password = std::mem::take(&mut profile.request.mysql.password);
    if profile.clear_password {
        delete_password(&profile.id);
    } else if !password.is_empty() {
        if profile.request.mysql.save_password {
            set_password(&profile.id, &password)?;
        }
    } else if !profile.request.mysql.save_password {
        // save_password toggled off → remove existing keyring entry
        delete_password(&profile.id);
    }
    // Extract SSH passphrase
    if let Some(ref mut ssh) = profile.request.ssh {
        let passphrase = std::mem::take(&mut ssh.passphrase);
        if profile.clear_ssh_passphrase {
            delete_ssh_passphrase(&profile.id);
        } else if !passphrase.is_empty() {
            if ssh.save_ssh_passphrase {
                set_ssh_passphrase(&profile.id, &passphrase)?;
            }
        } else if !ssh.save_ssh_passphrase {
            delete_ssh_passphrase(&profile.id);
        }
        // Extract SSH password
        let ssh_pw = std::mem::take(&mut ssh.ssh_password);
        if profile.clear_ssh_password {
            delete_ssh_password(&profile.id);
        } else if !ssh_pw.is_empty() {
            if ssh.save_ssh_password {
                set_ssh_password(&profile.id, &ssh_pw)?;
            }
        } else if !ssh.save_ssh_password {
            delete_ssh_password(&profile.id);
        }
    }
    let saved_id = profile.id.clone();
    let mut store = load_profiles(&app)?;
    let mut updated = false;
    for item in store.items.iter_mut() {
        if item.id == profile.id {
            *item = profile.clone();
            updated = true;
            break;
        }
    }
    if !updated {
        store.items.push(profile);
    }
    save_profiles(&app, &store)?;
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: Some(saved_id),
    })
}

#[tauri::command]
fn delete_profile(app: AppHandle, id: String) -> Result<ProfileListResponse, String> {
    let mut store = load_profiles(&app)?;
    store.items.retain(|item| item.id != id);
    delete_password(&id);
    delete_ssh_passphrase(&id);
    delete_ssh_password(&id);
    save_profiles(&app, &store)?;
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: None,
    })
}

#[tauri::command]
fn save_group(
    app: AppHandle,
    id: Option<String>,
    name: String,
) -> Result<ProfileListResponse, String> {
    if name.trim().is_empty() {
        return Err("Group name is empty".to_string());
    }
    let mut store = load_profiles(&app)?;
    if let Some(existing_id) = id {
        // Rename existing group
        if let Some(group) = store.groups.iter_mut().find(|g| g.id == existing_id) {
            group.name = name;
        } else {
            return Err("Group not found".to_string());
        }
    } else {
        // Create new group
        let max_order = store.groups.iter().map(|g| g.order).max().unwrap_or(0);
        let max_item_order = store
            .items
            .iter()
            .filter(|it| it.group_id.is_none())
            .map(|it| it.order)
            .max()
            .unwrap_or(0);
        let new_order = max_order.max(max_item_order) + 1000;
        store.groups.push(ProfileGroup {
            id: generate_profile_id(),
            name,
            order: new_order,
        });
    }
    save_profiles(&app, &store)?;
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: None,
    })
}

#[tauri::command]
fn delete_group(app: AppHandle, id: String) -> Result<ProfileListResponse, String> {
    let mut store = load_profiles(&app)?;
    store.groups.retain(|g| g.id != id);
    // Move children to root
    for item in store.items.iter_mut() {
        if item.group_id.as_deref() == Some(&id) {
            item.group_id = None;
        }
    }
    save_profiles(&app, &store)?;
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: None,
    })
}

#[tauri::command]
fn duplicate_profile(app: AppHandle, id: String) -> Result<ProfileListResponse, String> {
    let mut store = load_profiles(&app)?;
    let source = store
        .items
        .iter()
        .find(|it| it.id == id)
        .ok_or("Profile not found")?
        .clone();
    let max_order = store
        .items
        .iter()
        .filter(|it| it.group_id == source.group_id)
        .map(|it| it.order)
        .max()
        .unwrap_or(0);
    let new_id = generate_profile_id();
    // Copy password from source to new profile in keyring
    let source_pw = get_password(&id);
    if !source_pw.is_empty() {
        let _ = set_password(&new_id, &source_pw);
    }
    // Copy SSH passphrase
    let source_pp = get_ssh_passphrase(&id);
    if !source_pp.is_empty() {
        let _ = set_ssh_passphrase(&new_id, &source_pp);
    }
    // Copy SSH password
    let source_ssh_pw = get_ssh_password(&id);
    if !source_ssh_pw.is_empty() {
        let _ = set_ssh_password(&new_id, &source_ssh_pw);
    }
    let new_profile = ConnectionProfile {
        id: new_id,
        name: format!("{} (copy)", source.name),
        group_id: source.group_id,
        order: max_order + 1000,
        color: source.color,
        tags: source.tags,
        request: source.request,
        clear_password: false,
        clear_ssh_passphrase: false,
        clear_ssh_password: false,
    };
    store.items.push(new_profile);
    save_profiles(&app, &store)?;
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: None,
    })
}

#[derive(Debug, Deserialize)]
struct ProfilePatch {
    id: String,
    group_id: Option<String>,
    order: u32,
}

#[derive(Debug, Deserialize)]
struct GroupPatch {
    id: String,
    order: u32,
}

#[tauri::command]
fn reorder(
    app: AppHandle,
    profile_patches: Vec<ProfilePatch>,
    group_patches: Vec<GroupPatch>,
) -> Result<ProfileListResponse, String> {
    let mut store = load_profiles(&app)?;
    for patch in &profile_patches {
        if let Some(item) = store.items.iter_mut().find(|it| it.id == patch.id) {
            item.group_id = patch.group_id.clone();
            item.order = patch.order;
        }
    }
    for patch in &group_patches {
        if let Some(group) = store.groups.iter_mut().find(|g| g.id == patch.id) {
            group.order = patch.order;
        }
    }
    save_profiles(&app, &store)?;
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: None,
    })
}

#[tauri::command]
fn open_settings_window(
    app: AppHandle,
    id: Option<String>,
    group_id: Option<String>,
) -> Result<(), String> {
    let window = app
        .get_webview_window(WIN_SETTINGS)
        .ok_or("Settings window not found")?;
    window
        .emit("settings:open", json!({ "id": id, "group_id": group_id }))
        .map_err(|e| format!("Failed to send settings event: {e}"))?;
    window
        .show()
        .map_err(|e| format!("Failed to show settings window: {e}"))?;
    window
        .set_focus()
        .map_err(|e| format!("Failed to focus settings window: {e}"))?;
    Ok(())
}

#[tauri::command]
fn open_query_window(app: AppHandle, id: String) -> Result<(), String> {
    let window = app
        .get_webview_window(WIN_QUERY)
        .ok_or("Query window not found")?;
    window
        .emit("query:open", id)
        .map_err(|e| format!("Failed to send query event: {e}"))?;
    window
        .show()
        .map_err(|e| format!("Failed to show query window: {e}"))?;
    window
        .set_focus()
        .map_err(|e| format!("Failed to focus query window: {e}"))?;
    // Hide main (connections) window
    if let Some(main_win) = app.get_webview_window(WIN_MAIN) {
        let _ = main_win.hide();
    }
    Ok(())
}

#[tauri::command]
fn hide_window(window: Window) -> Result<(), String> {
    window
        .hide()
        .map_err(|e| format!("Failed to hide window: {e}"))
}

// ── Menu builders ──

#[tauri::command]
fn show_popup_menu(
    window: Window,
    lang: String,
    theme: String,
    notify: Option<bool>,
) -> Result<(), String> {
    let handle = window.app_handle();
    let menu = match window.label() {
        WIN_MAIN => build_main_menu(handle, &lang, &theme),
        WIN_QUERY => build_query_menu(handle, &lang, &theme, notify.unwrap_or(true)),
        WIN_SETTINGS => build_settings_menu(handle, &lang, &theme),
        _ => return Ok(()),
    }
    .map_err(|e| format!("{e}"))?;
    window.popup_menu(&menu).map_err(|e| format!("{e}"))?;
    Ok(())
}

// Open a URL in the default browser. Restricted to https, parsed with a real
// URL parser, and opened via tauri-plugin-opener (ShellExecuteW — no cmd.exe,
// so shell metacharacters in the URL cannot inject commands).
fn open_in_browser(url: &str) -> Result<(), String> {
    let parsed = tauri::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err("Only https URLs can be opened".into());
    }
    tauri_plugin_opener::open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| format!("Failed to open browser: {e}"))
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    open_in_browser(&url)
}

fn ml<'a>(lang: &str, key: &'a str) -> &'a str {
    match (lang, key) {
        ("ja", "file") => "ファイル",
        ("ja", "edit") => "編集",
        ("ja", "help") => "ヘルプ",
        ("ja", "query") => "クエリ",
        ("ja", "view") => "表示",
        ("ja", "new_profile") => "新規プロファイル",
        ("ja", "new_group") => "新規グループ",
        ("ja", "import") => "インポート...",
        ("ja", "export") => "エクスポート...",
        ("ja", "sync_settings") => "接続の同期...",
        ("ja", "exit") => "終了",
        ("ja", "github") => "GitHub リポジトリ",
        ("ja", "manual") => "マニュアル",
        ("ja", "check_update") => "アップデートを確認...",
        ("ja", "settings") => "設定",
        ("ja", "new_sql_tab") => "新規 SQL タブ",
        ("ja", "close_window") => "ウィンドウを閉じる",
        ("ja", "run") => "実行",
        ("ja", "run_all") => "すべて実行",
        ("ja", "cancel") => "キャンセル",
        ("ja", "format") => "整形",
        ("ja", "switch_db") => "データベース切替",
        ("ja", "theme") => "テーマ",
        ("ja", "theme_light") => "ライト",
        ("ja", "theme_dark") => "ダーク",
        ("ja", "language") => "言語",
        ("ja", "ai_settings") => "AI 設定",
        ("ja", "notify_query") => "クエリ完了を通知",
        ("ja", "test_connection") => "接続テスト",
        ("ja", "connect") => "接続",
        ("ja", "save") => "保存",
        ("ja", "delete") => "削除",
        (_, "file") => "File",
        (_, "edit") => "Edit",
        (_, "help") => "Help",
        (_, "query") => "Query",
        (_, "view") => "View",
        (_, "ai_settings") => "AI Settings",
        (_, "notify_query") => "Notify on Query Completion",
        (_, "settings") => "Settings",
        (_, "new_profile") => "New Profile",
        (_, "new_group") => "New Group",
        (_, "import") => "Import Profiles...",
        (_, "export") => "Export Profiles...",
        (_, "sync_settings") => "Sync Connections...",
        (_, "exit") => "Exit",
        (_, "github") => "GitHub Repository",
        (_, "manual") => "Manual",
        (_, "check_update") => "Check for Updates...",
        (_, "new_sql_tab") => "New SQL Tab",
        (_, "close_window") => "Close Window",
        (_, "run") => "Run",
        (_, "run_all") => "Run All",
        (_, "cancel") => "Cancel",
        (_, "format") => "Format",
        (_, "switch_db") => "Switch Database",
        (_, "theme") => "Theme",
        (_, "theme_light") => "Light",
        (_, "theme_dark") => "Dark",
        (_, "language") => "Language",
        (_, "test_connection") => "Test Connection",
        (_, "connect") => "Connect",
        (_, "save") => "Save",
        (_, "delete") => "Delete",
        _ => key,
    }
}

fn build_edit_submenu(handle: &AppHandle<Wry>, lang: &str) -> tauri::Result<Submenu<Wry>> {
    Submenu::with_items(
        handle,
        ml(lang, "edit"),
        true,
        &[
            &PredefinedMenuItem::undo(handle, None)?,
            &PredefinedMenuItem::redo(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::cut(handle, None)?,
            &PredefinedMenuItem::copy(handle, None)?,
            &PredefinedMenuItem::paste(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::select_all(handle, None)?,
        ],
    )
}

fn build_view_items(
    handle: &AppHandle<Wry>,
    lang: &str,
    theme: &str,
    prefix: &str,
) -> tauri::Result<[Box<dyn tauri::menu::IsMenuItem<Wry>>; 6]> {
    let is_light = theme != "dark";
    let is_ja = lang == "ja";
    Ok([
        Box::new(
            CheckMenuItemBuilder::with_id(format!("{prefix}:theme-light"), ml(lang, "theme_light"))
                .checked(is_light)
                .build(handle)?,
        ),
        Box::new(
            CheckMenuItemBuilder::with_id(format!("{prefix}:theme-dark"), ml(lang, "theme_dark"))
                .checked(!is_light)
                .build(handle)?,
        ),
        Box::new(PredefinedMenuItem::separator(handle)?),
        Box::new(
            CheckMenuItemBuilder::with_id(format!("{prefix}:lang-en"), "English")
                .checked(!is_ja)
                .build(handle)?,
        ),
        Box::new(
            CheckMenuItemBuilder::with_id(format!("{prefix}:lang-ja"), "日本語")
                .checked(is_ja)
                .build(handle)?,
        ),
        Box::new(PredefinedMenuItem::separator(handle)?),
    ])
}

fn build_view_submenu(
    handle: &AppHandle<Wry>,
    lang: &str,
    theme: &str,
    prefix: &str,
) -> tauri::Result<Submenu<Wry>> {
    let items = build_view_items(handle, lang, theme, prefix)?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = items.iter().map(|b| b.as_ref()).collect();
    Submenu::with_items(handle, ml(lang, "view"), true, &refs)
}

fn build_main_menu(handle: &AppHandle<Wry>, lang: &str, theme: &str) -> tauri::Result<Menu<Wry>> {
    let file_menu = Submenu::with_items(
        handle,
        ml(lang, "file"),
        true,
        &[
            &MenuItem::with_id(
                handle,
                "main:new-profile",
                ml(lang, "new_profile"),
                true,
                Some("CmdOrCtrl+N"),
            )?,
            &MenuItem::with_id(
                handle,
                "main:new-group",
                ml(lang, "new_group"),
                true,
                Some("CmdOrCtrl+Shift+N"),
            )?,
            &PredefinedMenuItem::separator(handle)?,
            &MenuItem::with_id(
                handle,
                "main:import",
                ml(lang, "import"),
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(
                handle,
                "main:export",
                ml(lang, "export"),
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(
                handle,
                "main:sync-settings",
                ml(lang, "sync_settings"),
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(handle)?,
            &MenuItem::with_id(handle, "main:exit", ml(lang, "exit"), true, None::<&str>)?,
        ],
    )?;
    let edit_menu = build_edit_submenu(handle, lang)?;
    let view_menu = build_view_submenu(handle, lang, theme, WIN_MAIN)?;
    let manual_item = MenuItem::with_id(
        handle,
        "main:manual",
        ml(lang, "manual"),
        true,
        None::<&str>,
    )?;
    let github_item = MenuItem::with_id(
        handle,
        "main:github",
        ml(lang, "github"),
        true,
        None::<&str>,
    )?;
    #[cfg(feature = "self-updater")]
    let help_menu = {
        let check_update_item = MenuItem::with_id(
            handle,
            "main:check-update",
            ml(lang, "check_update"),
            true,
            None::<&str>,
        )?;
        let sep = PredefinedMenuItem::separator(handle)?;
        Submenu::with_items(
            handle,
            ml(lang, "help"),
            true,
            &[&check_update_item, &sep, &manual_item, &github_item],
        )?
    };
    #[cfg(not(feature = "self-updater"))]
    let help_menu = Submenu::with_items(
        handle,
        ml(lang, "help"),
        true,
        &[&manual_item, &github_item],
    )?;
    Menu::with_items(handle, &[&file_menu, &edit_menu, &view_menu, &help_menu])
}

fn build_query_menu(
    handle: &AppHandle<Wry>,
    lang: &str,
    theme: &str,
    notify: bool,
) -> tauri::Result<Menu<Wry>> {
    let file_menu = Submenu::with_items(
        handle,
        ml(lang, "file"),
        true,
        &[
            &MenuItem::with_id(
                handle,
                "query:new-sql-tab",
                ml(lang, "new_sql_tab"),
                true,
                Some("CmdOrCtrl+T"),
            )?,
            &PredefinedMenuItem::separator(handle)?,
            &MenuItem::with_id(
                handle,
                "query:close",
                ml(lang, "close_window"),
                true,
                Some("CmdOrCtrl+W"),
            )?,
        ],
    )?;
    let edit_menu = build_edit_submenu(handle, lang)?;
    // Query view menu: Switch DB + shared theme/lang items
    let view_items = build_view_items(handle, lang, theme, WIN_QUERY)?;
    let switch_db = MenuItem::with_id(
        handle,
        "query:switch-db",
        ml(lang, "switch_db"),
        true,
        None::<&str>,
    )?;
    let ai_settings = MenuItem::with_id(
        handle,
        "query:ai-settings",
        ml(lang, "ai_settings"),
        true,
        None::<&str>,
    )?;
    let ai_sep = PredefinedMenuItem::separator(handle)?;
    let notify_item =
        CheckMenuItemBuilder::with_id("query:toggle-notify", ml(lang, "notify_query"))
            .checked(notify)
            .build(handle)?;
    let notify_sep = PredefinedMenuItem::separator(handle)?;
    let mut view_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
        vec![&switch_db, &ai_settings, &ai_sep, &notify_item, &notify_sep];
    for item in &view_items {
        view_refs.push(item.as_ref());
    }
    let view_menu = Submenu::with_items(handle, ml(lang, "view"), true, &view_refs)?;
    let query_menu = Submenu::with_items(
        handle,
        ml(lang, "query"),
        true,
        &[
            &MenuItem::with_id(
                handle,
                "query:run",
                ml(lang, "run"),
                true,
                Some("CmdOrCtrl+Enter"),
            )?,
            &MenuItem::with_id(
                handle,
                "query:run-all",
                ml(lang, "run_all"),
                true,
                Some("CmdOrCtrl+Shift+Enter"),
            )?,
            &MenuItem::with_id(
                handle,
                "query:cancel",
                ml(lang, "cancel"),
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(handle)?,
            &MenuItem::with_id(
                handle,
                "query:format",
                ml(lang, "format"),
                true,
                Some("CmdOrCtrl+Shift+F"),
            )?,
        ],
    )?;
    Menu::with_items(handle, &[&file_menu, &edit_menu, &view_menu, &query_menu])
}

fn build_settings_menu(
    handle: &AppHandle<Wry>,
    lang: &str,
    theme: &str,
) -> tauri::Result<Menu<Wry>> {
    let edit_menu = build_edit_submenu(handle, lang)?;
    let view_menu = build_view_submenu(handle, lang, theme, WIN_SETTINGS)?;
    let settings_menu = Submenu::with_items(
        handle,
        ml(lang, "settings"),
        true,
        &[
            &MenuItem::with_id(
                handle,
                "settings:test-connection",
                ml(lang, "test_connection"),
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(
                handle,
                "settings:connect",
                ml(lang, "connect"),
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(handle)?,
            &MenuItem::with_id(
                handle,
                "settings:save",
                ml(lang, "save"),
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(
                handle,
                "settings:delete",
                ml(lang, "delete"),
                true,
                None::<&str>,
            )?,
        ],
    )?;
    Menu::with_items(handle, &[&edit_menu, &view_menu, &settings_menu])
}

/// Set menus once at startup to register accelerators, then hide menu bars.
fn setup_menus(handle: &AppHandle<Wry>) -> tauri::Result<()> {
    let main_menu = build_main_menu(handle, "en", "light")?;
    // This hidden menu only registers accelerators; the notify checkmark is set
    // for real when the popup menu is opened (show_popup_menu).
    let query_menu = build_query_menu(handle, "en", "light", true)?;
    let settings_menu = build_settings_menu(handle, "en", "light")?;
    if let Some(w) = handle.get_webview_window(WIN_MAIN) {
        let _ = w.set_menu(main_menu);
        let _ = w.hide_menu();
    }
    if let Some(w) = handle.get_webview_window(WIN_QUERY) {
        let _ = w.set_menu(query_menu);
        let _ = w.hide_menu();
    }
    if let Some(w) = handle.get_webview_window(WIN_SETTINGS) {
        let _ = w.set_menu(settings_menu);
        let _ = w.hide_menu();
    }
    Ok(())
}

#[cfg(feature = "self-updater")]
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<bool, String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    if let Some(update) = update {
        let _ = app.emit(
            "update-available",
            serde_json::json!({
              "version": update.version
            }),
        );
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(feature = "self-updater")]
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    if let Some(update) = update {
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| e.to_string())?;
        app.restart();
    }
    Ok(())
}

#[cfg(not(feature = "self-updater"))]
#[tauri::command]
async fn check_update(_app: AppHandle) -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(feature = "self-updater"))]
#[tauri::command]
async fn install_update(_app: AppHandle) -> Result<(), String> {
    Ok(())
}

// ── Docker commands ──

#[derive(Debug, Deserialize, Serialize, Clone)]
struct DockerConnectionInfo {
    host: String,
    port: u16,
    user: String,
    password: String,
    name: String,
    ssl_mode: Option<String>,
    tunnel_container_id: Option<String>,
}

#[cfg(feature = "docker")]
async fn connect_docker() -> Result<bollard::Docker, String> {
    // 1. DOCKER_HOST env var / platform default (named pipe on Windows)
    if let Ok(docker) = bollard::Docker::connect_with_local_defaults() {
        if docker.ping().await.is_ok() {
            return Ok(docker);
        }
    }
    // 2. TCP fallback — covers WSL2 dockerd with tcp://127.0.0.1:2375
    for port in [2375, 2376] {
        let url = format!("tcp://127.0.0.1:{port}");
        if let Ok(docker) = bollard::Docker::connect_with_http_defaults() {
            // connect_with_http_defaults uses DOCKER_HOST; try explicit URL
            drop(docker);
        }
        if let Ok(docker) =
            bollard::Docker::connect_with_http(&url, 4, bollard::API_DEFAULT_VERSION)
        {
            if docker.ping().await.is_ok() {
                return Ok(docker);
            }
        }
    }
    Err("Docker is not reachable".to_string())
}

#[cfg(feature = "docker")]
#[tauri::command]
async fn docker_available() -> Result<bool, String> {
    Ok(connect_docker().await.is_ok())
}

#[cfg(feature = "docker")]
#[tauri::command]
async fn docker_list_containers() -> Result<Vec<docker::discovery::DockerContainer>, String> {
    let docker = connect_docker().await?;
    docker::discovery::discover_mysql_containers(&docker).await
}

#[cfg(feature = "docker")]
#[tauri::command]
async fn docker_create_tunnel(
    container_id: String,
    port: u16,
) -> Result<docker::tunnel::TunnelInfo, String> {
    let docker = connect_docker().await?;
    docker::tunnel::ensure_socat_image(&docker).await?;
    docker::tunnel::create_tunnel(&docker, &container_id, port).await
}

#[cfg(feature = "docker")]
#[tauri::command]
async fn docker_stop_tunnel(container_id: String) -> Result<(), String> {
    let docker = connect_docker().await?;
    docker::tunnel::stop_tunnel(&docker, &container_id).await
}

#[cfg(feature = "docker")]
#[tauri::command]
async fn docker_cleanup_tunnels() -> Result<(), String> {
    let docker = connect_docker().await?;
    docker::tunnel::cleanup_all_tunnels(&docker).await
}

#[cfg(feature = "docker")]
#[tauri::command]
fn open_docker_query_window(app: AppHandle, info: DockerConnectionInfo) -> Result<(), String> {
    let window = app
        .get_webview_window(WIN_QUERY)
        .ok_or("Query window not found")?;
    window
        .emit("query:docker-open", &info)
        .map_err(|e| format!("Failed to send docker query event: {e}"))?;
    window
        .show()
        .map_err(|e| format!("Failed to show query window: {e}"))?;
    window
        .set_focus()
        .map_err(|e| format!("Failed to focus query window: {e}"))?;
    if let Some(main_win) = app.get_webview_window(WIN_MAIN) {
        let _ = main_win.hide();
    }
    Ok(())
}

// Stubs when docker feature is disabled
#[cfg(not(feature = "docker"))]
#[tauri::command]
async fn docker_available() -> Result<bool, String> {
    Ok(false)
}

#[cfg(not(feature = "docker"))]
#[tauri::command]
async fn docker_list_containers() -> Result<Vec<()>, String> {
    Ok(vec![])
}

#[cfg(not(feature = "docker"))]
#[tauri::command]
async fn docker_create_tunnel(
    _container_id: String,
    _port: u16,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({}))
}

#[cfg(not(feature = "docker"))]
#[tauri::command]
async fn docker_stop_tunnel(_container_id: String) -> Result<(), String> {
    Ok(())
}

#[cfg(not(feature = "docker"))]
#[tauri::command]
async fn docker_cleanup_tunnels() -> Result<(), String> {
    Ok(())
}

#[cfg(not(feature = "docker"))]
#[tauri::command]
fn open_docker_query_window(_app: AppHandle, _info: DockerConnectionInfo) -> Result<(), String> {
    Ok(())
}

fn main() {
    // `mut` is only used when the self-updater feature adds another plugin below.
    #[allow(unused_mut)]
    // Persist & restore window size / position / maximized only (#42). VISIBLE etc. are
    // excluded so the show/hide pattern isn't overridden; the fixed-size settings dialog
    // is denylisted so its geometry is never forced.
    let mut builder = tauri::Builder::default().plugin(
        tauri_plugin_window_state::Builder::default()
            .with_state_flags(
                tauri_plugin_window_state::StateFlags::SIZE
                    | tauri_plugin_window_state::StateFlags::POSITION
                    | tauri_plugin_window_state::StateFlags::MAXIMIZED,
            )
            .with_denylist(&["settings"])
            .build(),
    );
    // Desktop notifications for long-running query completion (#43).
    builder = builder.plugin(tauri_plugin_notification::init());
    #[cfg(feature = "self-updater")]
    {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }
    builder
        .setup(|app| {
            setup_menus(app.handle())?;
            #[cfg(feature = "docker")]
            {
                tauri::async_runtime::spawn(async {
                    if let Ok(docker) = connect_docker().await {
                        let _ = crate::docker::tunnel::cleanup_all_tunnels(&docker).await;
                    }
                });
            }
            #[cfg(feature = "self-updater")]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Delay to avoid slowing down app startup
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    use tauri_plugin_updater::UpdaterExt;
                    if let Ok(updater) = handle.updater() {
                        if let Ok(Some(update)) = updater.check().await {
                            let _ = handle.emit(
                                "update-available",
                                serde_json::json!({ "version": update.version }),
                            );
                        }
                    }
                });
            }
            Ok(())
        })
        .on_menu_event(|app, event| {
            let id = event.id().0.as_str();
            match id {
                "main:exit" => {
                    // std::process::exit bypasses the plugin's save-on-exit hook,
                    // so persist window state explicitly first (#42).
                    {
                        use tauri_plugin_window_state::{AppHandleExt, StateFlags};
                        let _ = app.save_window_state(StateFlags::all());
                    }
                    #[cfg(feature = "docker")]
                    {
                        tauri::async_runtime::spawn(async {
                            if let Ok(docker) = connect_docker().await {
                                let _ = crate::docker::tunnel::cleanup_all_tunnels(&docker).await;
                            }
                            std::process::exit(0);
                        });
                    }
                    #[cfg(not(feature = "docker"))]
                    std::process::exit(0);
                }
                "main:github" => {
                    let _ = open_in_browser("https://github.com/kan/musql");
                }
                "main:manual" => {
                    let _ = app.emit_to(
                        EventTarget::webview_window(WIN_MAIN),
                        "menu:action",
                        "manual",
                    );
                }
                #[cfg(feature = "self-updater")]
                "main:check-update" => {
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        use tauri_plugin_updater::UpdaterExt;
                        match handle.updater() {
                            Ok(updater) => match updater.check().await {
                                Ok(Some(update)) => {
                                    let _ = handle.emit(
                                        "update-available",
                                        serde_json::json!({
                                          "version": update.version
                                        }),
                                    );
                                }
                                Ok(None) => {
                                    let _ = handle.emit_to(
                                        EventTarget::webview_window(WIN_MAIN),
                                        "menu:action",
                                        "no-update",
                                    );
                                }
                                Err(_) => {
                                    let _ = handle.emit_to(
                                        EventTarget::webview_window(WIN_MAIN),
                                        "menu:action",
                                        "no-update",
                                    );
                                }
                            },
                            Err(_) => {
                                let _ = handle.emit_to(
                                    EventTarget::webview_window(WIN_MAIN),
                                    "menu:action",
                                    "no-update",
                                );
                            }
                        }
                    });
                }
                "query:close" => {
                    if let Some(w) = app.get_webview_window(WIN_QUERY) {
                        let _ = w.emit("query:reset", ());
                        let _ = w.hide();
                    }
                    if let Some(w) = app.get_webview_window(WIN_MAIN) {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                _ => {
                    if let Some((window_label, action)) = id.split_once(':') {
                        let _ = app.emit_to(
                            EventTarget::webview_window(window_label),
                            "menu:action",
                            action,
                        );
                    }
                }
            }
        })
        .manage(Arc::new(Mutex::new(None::<ConnectionCache>)))
        .manage(ActiveWindow(Mutex::new(WIN_MAIN.to_string())))
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(true) => {
                if let Some(state) = window.try_state::<ActiveWindow>() {
                    if let Ok(mut label) = state.0.lock() {
                        *label = window.label().to_string();
                    }
                }
            }
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() != WIN_MAIN => {
                api.prevent_close();
                let is_query = window.label() == WIN_QUERY;
                if is_query {
                    let _ = window.emit("query:reset", ());
                }
                let _ = window.hide();
                if is_query {
                    if let Some(main_win) = window.app_handle().get_webview_window(WIN_MAIN) {
                        let _ = main_win.show();
                        let _ = main_win.set_focus();
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            test_connection,
            run_query,
            list_profiles,
            save_profile,
            delete_profile,
            save_group,
            delete_group,
            duplicate_profile,
            reorder,
            open_settings_window,
            open_query_window,
            hide_window,
            disconnect_pool,
            cancel_query,
            export_file,
            pick_file,
            list_ssh_config_hosts,
            resolve_ssh_config,
            has_password,
            has_ssh_passphrase,
            has_ssh_password,
            export_profiles,
            import_profiles,
            get_sync_path,
            set_sync_path,
            sync_import,
            sync_export,
            pick_sync_path,
            show_popup_menu,
            check_update,
            install_update,
            ai_complete,
            ai_assist,
            save_ai_api_key,
            has_ai_api_key,
            clear_schema_cache,
            docker_available,
            docker_list_containers,
            docker_create_tunnel,
            docker_stop_tunnel,
            docker_cleanup_tunnels,
            open_docker_query_window,
            get_docker_password,
            save_docker_password,
            op_available,
            op_read_secret,
            op_list_items,
            op_list_fields,
            open_external
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            let _ = writeln_fallback(&format!("error while running tauri application: {e}"));
            panic!("error while running tauri application: {e}");
        });
}

fn writeln_fallback(message: &str) -> io::Result<()> {
    use std::io::Write;
    let mut stderr = io::stderr();
    stderr.write_all(message.as_bytes())?;
    stderr.write_all(b"\n")?;
    stderr.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── escape_identifier ──────────────────────────────────────────

    #[test]
    fn escape_identifier_normal() {
        assert_eq!(escape_identifier("users"), "`users`");
    }

    #[test]
    fn escape_identifier_backtick() {
        assert_eq!(escape_identifier("my`table"), "`my``table`");
    }

    #[test]
    fn escape_identifier_empty() {
        assert_eq!(escape_identifier(""), "``");
    }

    #[test]
    fn escape_identifier_unicode() {
        assert_eq!(escape_identifier("テーブル"), "`テーブル`");
    }

    // ── parse_ssh_config_host ──────────────────────────────────────

    #[test]
    fn parse_ssh_config_all_fields() {
        let config = "\
Host myserver
  HostName 10.0.0.1
  Port 2222
  User deploy
  IdentityFile ~/.ssh/id_deploy
";
        let (host, port, user, key) = parse_ssh_config_host(config, "myserver", "/home/test");
        assert_eq!(host, "10.0.0.1");
        assert_eq!(port, 2222);
        assert_eq!(user.as_deref(), Some("deploy"));
        assert_eq!(key.as_deref(), Some("/home/test/.ssh/id_deploy"));
    }

    #[test]
    fn parse_ssh_config_alias_not_found() {
        let config = "\
Host other
  HostName 10.0.0.2
  Port 3333
";
        let (host, port, user, key) = parse_ssh_config_host(config, "myserver", "/home/test");
        assert_eq!(host, "myserver");
        assert_eq!(port, 22);
        assert!(user.is_none());
        assert!(key.is_none());
    }

    #[test]
    fn parse_ssh_config_multiple_host_blocks() {
        let config = "\
Host first
  HostName 10.0.0.1
  User alice

Host second
  HostName 10.0.0.2
  User bob
  Port 2222
";
        let (host, _port, user, _key) = parse_ssh_config_host(config, "second", "/home/test");
        assert_eq!(host, "10.0.0.2");
        assert_eq!(user.as_deref(), Some("bob"));
    }

    #[test]
    fn parse_ssh_config_case_insensitive_keys() {
        let config = "\
host myserver
  hostname 10.0.0.1
  port 2222
  user deploy
  identityfile /keys/id_rsa
";
        let (host, port, user, key) = parse_ssh_config_host(config, "myserver", "/home/test");
        assert_eq!(host, "10.0.0.1");
        assert_eq!(port, 2222);
        assert_eq!(user.as_deref(), Some("deploy"));
        assert_eq!(key.as_deref(), Some("/keys/id_rsa"));
    }

    #[test]
    fn parse_ssh_config_tilde_expansion() {
        let config = "\
Host myserver
  IdentityFile ~/my_key
";
        let (_host, _port, _user, key) = parse_ssh_config_host(config, "myserver", "/home/test");
        assert_eq!(key.as_deref(), Some("/home/test/my_key"));
    }

    // ── connection_fingerprint ─────────────────────────────────────

    fn make_request(
        host: &str,
        port: u16,
        user: &str,
        ssh: Option<SshConfig>,
    ) -> ConnectionRequest {
        ConnectionRequest {
            mysql: MySqlConfig {
                host: host.to_string(),
                port,
                database: None,
                username: user.to_string(),
                password: String::new(),
                ssl_mode: "DISABLED".to_string(),
                tls_ca_cert_path: None,
                save_password: true,
                op_ref: None,
                tls_enabled: false,
                tls_skip_verify: false,
            },
            ssh,
        }
    }

    #[test]
    fn fingerprint_no_ssh() {
        let req = make_request("127.0.0.1", 3306, "root", None);
        let fp = connection_fingerprint(&req);
        assert!(fp.contains("127.0.0.1:3306:root"));
        assert!(fp.ends_with('|'));
    }

    #[test]
    fn fingerprint_with_ssh_manual() {
        let ssh = SshConfig {
            enabled: true,
            host: "bastion.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            private_key_path: Some("/keys/id_rsa".to_string()),
            config_host: None,
            passphrase: String::new(),
            auth_method: "key".to_string(),
            ssh_password: String::new(),
            save_ssh_password: true,
            save_ssh_passphrase: true,
            op_passphrase_ref: None,
            op_password_ref: None,
        };
        let req = make_request("127.0.0.1", 3306, "root", Some(ssh));
        let fp = connection_fingerprint(&req);
        assert!(fp.contains("ssh:bastion.example.com:22:deploy:/keys/id_rsa:key"));
    }

    #[test]
    fn fingerprint_same_config_equal() {
        let req1 = make_request("127.0.0.1", 3306, "root", None);
        let req2 = make_request("127.0.0.1", 3306, "root", None);
        assert_eq!(connection_fingerprint(&req1), connection_fingerprint(&req2));
    }

    #[test]
    fn fingerprint_different_config_not_equal() {
        let req1 = make_request("127.0.0.1", 3306, "root", None);
        let req2 = make_request("127.0.0.1", 3307, "root", None);
        assert_ne!(connection_fingerprint(&req1), connection_fingerprint(&req2));
    }

    // ── generate_profile_id ────────────────────────────────────────

    #[test]
    fn generate_profile_id_unique() {
        let ids: Vec<String> = (0..10).map(|_| generate_profile_id()).collect();
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j], "IDs should be unique");
            }
        }
    }

    #[test]
    fn generate_profile_id_prefix() {
        let id = generate_profile_id();
        assert!(id.starts_with('p'), "ID should start with 'p': {id}");
    }

    // ── ai_keyring_key ───────────────────────────────────────────────

    #[test]
    fn ai_keyring_key_claude() {
        assert_eq!(ai_keyring_key(&AiProvider::Claude), "ai:claude");
    }

    #[test]
    fn ai_keyring_key_openai() {
        assert_eq!(ai_keyring_key(&AiProvider::OpenAi), "ai:openai");
    }

    #[test]
    fn ai_keyring_key_gemini() {
        assert_eq!(ai_keyring_key(&AiProvider::Gemini), "ai:gemini");
    }

    // ── AiProvider serde ─────────────────────────────────────────────

    #[test]
    fn ai_provider_deserialize() {
        let p: AiProvider = serde_json::from_str("\"claude\"").unwrap();
        assert!(matches!(p, AiProvider::Claude));
        let p: AiProvider = serde_json::from_str("\"openai\"").unwrap();
        assert!(matches!(p, AiProvider::OpenAi));
        let p: AiProvider = serde_json::from_str("\"gemini\"").unwrap();
        assert!(matches!(p, AiProvider::Gemini));
    }

    // ── build_ai_prompt ──────────────────────────────────────────────

    #[test]
    fn build_ai_prompt_basic() {
        let schema = SchemaInfo {
            database: "testdb".to_string(),
            tables: vec![SchemaTable {
                name: "users".to_string(),
                columns: vec![
                    SchemaColumn {
                        name: "id".to_string(),
                        data_type: "int".to_string(),
                        column_key: "PRI".to_string(),
                        is_nullable: "NO".to_string(),
                    },
                    SchemaColumn {
                        name: "name".to_string(),
                        data_type: "varchar".to_string(),
                        column_key: "".to_string(),
                        is_nullable: "YES".to_string(),
                    },
                ],
            }],
        };
        let prompt = build_ai_prompt(&schema, "SELECT ", "");
        assert!(prompt.contains("testdb"));
        assert!(prompt.contains("-- users"));
        assert!(prompt.contains("id int PK"));
        assert!(prompt.contains("name varchar"));
        assert!(prompt.contains("SELECT "));
        assert!(prompt.contains("[CURSOR]"));
    }

    #[test]
    fn build_ai_prompt_truncation() {
        let schema = SchemaInfo {
            database: "bigdb".to_string(),
            tables: (0..200)
                .map(|i| SchemaTable {
                    name: format!("table_{i}"),
                    columns: (0..20)
                        .map(|j| SchemaColumn {
                            name: format!("column_{j}_with_a_longer_name"),
                            data_type: "varchar".to_string(),
                            column_key: "".to_string(),
                            is_nullable: "YES".to_string(),
                        })
                        .collect(),
                })
                .collect(),
        };
        let prompt = build_ai_prompt(&schema, "SELECT ", "");
        assert!(prompt.contains("(truncated)"));
    }

    // ── 1Password resolution gating (#80) ────────────────────────────
    //
    // These assert that a credential which cannot be in play never reaches the CLI. The
    // reference points at a vault that does not exist, so a broken gate surfaces as an
    // Err rather than a silent extra process. No keyring entry exists for this id, so the
    // keyring lookup in between returns empty either way.

    const NO_SUCH_REF: &str = "op://NoSuchVault/NoSuchItem/password";
    const UNUSED_ID: &str = "p_test_op_gating_no_such_profile";

    fn ssh_with_refs(enabled: bool, auth_method: &str) -> SshConfig {
        let mut ssh = make_ssh(None);
        ssh.enabled = enabled;
        ssh.auth_method = auth_method.to_string();
        ssh.op_passphrase_ref = Some(NO_SUCH_REF.to_string());
        ssh.op_password_ref = Some(NO_SUCH_REF.to_string());
        ssh
    }

    #[test]
    fn resolve_ssh_secrets_skip_when_tunnel_disabled() {
        // The settings form always submits an ssh block, so a disabled tunnel can still
        // carry references left over from when it was on.
        let mut req = make_request("127.0.0.1", 3306, "root", Some(ssh_with_refs(false, "key")));
        resolve_ssh_passphrase(&mut req, Some(UNUSED_ID)).unwrap();
        resolve_ssh_password(&mut req, Some(UNUSED_ID)).unwrap();
        let ssh = req.ssh.unwrap();
        assert!(ssh.passphrase.is_empty());
        assert!(ssh.ssh_password.is_empty());
    }

    #[test]
    fn resolve_ssh_secrets_skip_the_unused_auth_method() {
        let mut req = make_request(
            "127.0.0.1",
            3306,
            "root",
            Some(ssh_with_refs(true, "password")),
        );
        resolve_ssh_passphrase(&mut req, Some(UNUSED_ID)).unwrap();
        assert!(req.ssh.as_ref().unwrap().passphrase.is_empty());

        let mut req = make_request("127.0.0.1", 3306, "root", Some(ssh_with_refs(true, "key")));
        resolve_ssh_password(&mut req, Some(UNUSED_ID)).unwrap();
        assert!(req.ssh.as_ref().unwrap().ssh_password.is_empty());
    }

    #[test]
    fn resolve_password_skips_op_when_not_persisting() {
        // Nowhere to cache the result means 1Password would be hit on every connection.
        let mut req = make_request("127.0.0.1", 3306, "root", None);
        req.mysql.op_ref = Some(NO_SUCH_REF.to_string());
        req.mysql.save_password = false;
        resolve_password(&mut req, Some(UNUSED_ID)).unwrap();
        assert!(req.mysql.password.is_empty());
    }

    #[test]
    fn resolve_password_prefers_a_value_already_on_the_request() {
        // What the per-connection prompt collected wins over both keyring and 1Password.
        let mut req = make_request("127.0.0.1", 3306, "root", None);
        req.mysql.op_ref = Some(NO_SUCH_REF.to_string());
        req.mysql.password = "typed-at-the-prompt".to_string();
        resolve_password(&mut req, Some(UNUSED_ID)).unwrap();
        assert_eq!(req.mysql.password, "typed-at-the-prompt");
    }

    #[test]
    fn resolve_password_without_a_reference_never_calls_op() {
        let mut req = make_request("127.0.0.1", 3306, "root", None);
        resolve_password(&mut req, Some(UNUSED_ID)).unwrap();
        assert!(req.mysql.password.is_empty());
    }

    // ── profile sync sanitization (#80) ──────────────────────────────

    fn make_ssh(private_key_path: Option<&str>) -> SshConfig {
        SshConfig {
            enabled: true,
            host: "bastion.example.com".to_string(),
            port: 22,
            username: "deploy".to_string(),
            private_key_path: private_key_path.map(str::to_string),
            config_host: Some("myserver".to_string()),
            passphrase: String::new(),
            auth_method: "key".to_string(),
            ssh_password: String::new(),
            save_ssh_password: true,
            save_ssh_passphrase: true,
            op_passphrase_ref: None,
            op_password_ref: None,
        }
    }

    fn make_profile(id: &str, ssh: Option<SshConfig>) -> ConnectionProfile {
        ConnectionProfile {
            id: id.to_string(),
            name: format!("profile {id}"),
            group_id: None,
            order: 0,
            color: None,
            tags: vec![],
            request: make_request("127.0.0.1", 3306, "root", ssh),
            clear_password: false,
            clear_ssh_passphrase: false,
            clear_ssh_password: false,
        }
    }

    #[test]
    fn sanitized_store_json_strips_machine_local_paths() {
        let mut profile = make_profile(
            "p1",
            Some(make_ssh(Some("C:\\Users\\alice\\.ssh\\id_ed25519"))),
        );
        profile.request.mysql.tls_ca_cert_path =
            Some("C:\\Users\\alice\\certs\\ca.pem".to_string());
        let store = ConnectionProfileStore {
            version: 1,
            groups: vec![],
            items: vec![profile],
        };

        let json = sanitized_store_json(&store).unwrap();
        assert!(
            !json.contains("alice"),
            "machine-local paths leaked: {json}"
        );
        // Non-path settings still sync.
        assert!(json.contains("bastion.example.com"));
        assert!(json.contains("myserver"));
    }

    #[test]
    fn sanitized_store_json_strips_secrets() {
        let mut profile = make_profile("p1", Some(make_ssh(None)));
        profile.request.mysql.password = "mysql-secret".to_string();
        let ssh = profile.request.ssh.as_mut().unwrap();
        ssh.passphrase = "passphrase-secret".to_string();
        ssh.ssh_password = "ssh-secret".to_string();
        let store = ConnectionProfileStore {
            version: 1,
            groups: vec![],
            items: vec![profile],
        };

        let json = sanitized_store_json(&store).unwrap();
        assert!(!json.contains("secret"), "secret leaked: {json}");
    }

    #[test]
    fn scrub_incoming_profile_clears_secrets_and_flags() {
        let mut profile = make_profile("p1", Some(make_ssh(None)));
        profile.request.mysql.password = "pw".to_string();
        profile.clear_password = true;
        profile.clear_ssh_passphrase = true;
        profile.clear_ssh_password = true;
        {
            let ssh = profile.request.ssh.as_mut().unwrap();
            ssh.passphrase = "pp".to_string();
            ssh.ssh_password = "sp".to_string();
        }

        scrub_incoming_profile(&mut profile);

        assert!(profile.request.mysql.password.is_empty());
        let ssh = profile.request.ssh.as_ref().unwrap();
        assert!(ssh.passphrase.is_empty());
        assert!(ssh.ssh_password.is_empty());
        assert!(!profile.clear_password);
        assert!(!profile.clear_ssh_passphrase);
        assert!(!profile.clear_ssh_password);
    }

    #[test]
    fn carry_over_keeps_local_paths() {
        let mut local = make_profile("p1", Some(make_ssh(Some("/home/local/.ssh/id_rsa"))));
        local.request.mysql.tls_ca_cert_path = Some("/home/local/ca.pem".to_string());
        // The sync file has these stripped, so the incoming side is empty.
        let mut incoming = make_profile("p1", Some(make_ssh(None)));

        carry_over_machine_local_paths(&local, &mut incoming);

        assert_eq!(
            incoming.request.ssh.unwrap().private_key_path.as_deref(),
            Some("/home/local/.ssh/id_rsa")
        );
        assert_eq!(
            incoming.request.mysql.tls_ca_cert_path.as_deref(),
            Some("/home/local/ca.pem")
        );
    }

    #[test]
    fn carry_over_local_wins_over_incoming() {
        let local = make_profile("p1", Some(make_ssh(Some("/home/local/.ssh/id_rsa"))));
        let mut incoming = make_profile("p1", Some(make_ssh(Some("/home/other/.ssh/id_rsa"))));

        carry_over_machine_local_paths(&local, &mut incoming);

        assert_eq!(
            incoming.request.ssh.unwrap().private_key_path.as_deref(),
            Some("/home/local/.ssh/id_rsa")
        );
    }

    // No fallback: an unset local path stays unset rather than adopting the file's value,
    // which would otherwise re-import another machine's path from a pre-#80 sync file.
    #[test]
    fn carry_over_clears_when_local_has_no_path() {
        let local = make_profile("p1", Some(make_ssh(None)));
        let mut incoming = make_profile("p1", Some(make_ssh(Some("/home/other/id_rsa"))));
        incoming.request.mysql.tls_ca_cert_path = Some("/home/other/ca.pem".to_string());

        carry_over_machine_local_paths(&local, &mut incoming);

        assert!(incoming.request.ssh.unwrap().private_key_path.is_none());
        assert!(incoming.request.mysql.tls_ca_cert_path.is_none());
    }

    // A sync file written before #80 still contains the origin machine's paths.
    #[test]
    fn legacy_sync_file_paths_never_reach_the_local_store() {
        let legacy = r#"{
          "version": 1,
          "groups": [],
          "items": [{
            "id": "p1",
            "name": "prod",
            "request": {
              "mysql": {
                "host": "db.example.com", "port": 3306, "database": null,
                "username": "root", "ssl_mode": "VERIFY_CA",
                "tls_ca_cert_path": "C:\\Users\\alice\\ca.pem", "save_password": true
              },
              "ssh": {
                "enabled": true, "host": "bastion", "port": 22, "username": "deploy",
                "private_key_path": "C:\\Users\\alice\\id_ed25519",
                "auth_method": "key"
              }
            }
          }]
        }"#;
        let incoming: ConnectionProfileStore = serde_json::from_str(legacy).unwrap();
        let mut it = incoming.items.into_iter().next().unwrap();

        // New profile on this machine: scrubbing alone must drop the foreign paths.
        scrub_incoming_profile(&mut it);
        assert!(it.request.mysql.tls_ca_cert_path.is_none());
        assert!(it.request.ssh.as_ref().unwrap().private_key_path.is_none());
        // Everything else still arrives.
        assert_eq!(it.request.mysql.host, "db.example.com");
        assert_eq!(it.request.mysql.ssl_mode, "VERIFY_CA");
    }

    // Machine A exports, machine B imports: B must keep its own key/cert paths while
    // picking up every other change A made. This is the regression the issue is about.
    #[test]
    fn sync_roundtrip_preserves_local_paths_and_applies_remote_edits() {
        let mut on_machine_a =
            make_profile("p1", Some(make_ssh(Some("C:\\Users\\alice\\id_ed25519"))));
        on_machine_a.name = "renamed on A".to_string();
        on_machine_a.request.mysql.host = "db.example.com".to_string();
        on_machine_a.request.mysql.tls_ca_cert_path = Some("C:\\Users\\alice\\ca.pem".to_string());
        let store_a = ConnectionProfileStore {
            version: 1,
            groups: vec![],
            items: vec![on_machine_a],
        };

        // A writes the sync file; B reads it back.
        let json = sanitized_store_json(&store_a).unwrap();
        let incoming: ConnectionProfileStore = serde_json::from_str(&json).unwrap();

        let mut on_machine_b =
            make_profile("p1", Some(make_ssh(Some("/home/bob/.ssh/id_ed25519"))));
        on_machine_b.request.mysql.tls_ca_cert_path = Some("/home/bob/ca.pem".to_string());

        let mut merged = incoming.items.into_iter().next().unwrap();
        scrub_incoming_profile(&mut merged);
        carry_over_machine_local_paths(&on_machine_b, &mut merged);

        // A's edits land …
        assert_eq!(merged.name, "renamed on A");
        assert_eq!(merged.request.mysql.host, "db.example.com");
        // … but B keeps its own filesystem paths.
        assert_eq!(
            merged.request.ssh.unwrap().private_key_path.as_deref(),
            Some("/home/bob/.ssh/id_ed25519")
        );
        assert_eq!(
            merged.request.mysql.tls_ca_cert_path.as_deref(),
            Some("/home/bob/ca.pem")
        );
    }

    #[test]
    fn carry_over_handles_missing_ssh_on_either_side() {
        // Local has no SSH block at all: there is no local path to carry, so the
        // incoming one is dropped rather than inherited.
        let local = make_profile("p1", None);
        let mut incoming = make_profile("p1", Some(make_ssh(Some("/home/other/id_rsa"))));
        carry_over_machine_local_paths(&local, &mut incoming);
        assert!(incoming.request.ssh.unwrap().private_key_path.is_none());

        // Incoming has no SSH block: nothing to write into, and no panic.
        let local = make_profile("p1", Some(make_ssh(Some("/home/local/id_rsa"))));
        let mut incoming = make_profile("p1", None);
        carry_over_machine_local_paths(&local, &mut incoming);
        assert!(incoming.request.ssh.is_none());
    }
}
