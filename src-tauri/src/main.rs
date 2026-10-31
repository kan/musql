#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mysql::{prelude::Queryable, OptsBuilder, Pool, PooledConn, Row, SslOpts, Value};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
// For `write!` into a `String` (which cannot fail).
use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::menu::{
    CheckMenuItem, CheckMenuItemBuilder, Menu, MenuItem, PredefinedMenuItem, Submenu,
};
use tauri::{AppHandle, Emitter, EventTarget, Manager, Window, Wry};

mod app_log;
#[cfg(test)]
mod credits;
#[cfg(feature = "docker")]
mod docker;
#[cfg(test)]
mod docs_consistency;
mod onepassword;
mod postgres_client;
mod redis_client;
mod sqlite_client;
mod sync_folder;
mod tableplus;
mod toast;
mod vdesk;
#[cfg(test)]
mod verify;

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MySqlConfig {
    host: String,
    port: u16,
    database: Option<String>,
    username: String,
    #[serde(default, skip_serializing)]
    password: String,
    #[serde(default)]
    ssl_mode: SslMode,
    #[serde(default)]
    tls_ca_cert_path: Option<String>,
    #[serde(default = "default_true")]
    save_password: bool, // false = don't persist to keyring (prompt each time)
    // 1Password secret reference (op://vault/item/field) used to seed / refresh the
    // keyring entry. A pointer, not a secret, so it syncs with the profile (#80).
    #[serde(default)]
    op_ref: Option<String>,
    /// The database file of a SQLite connection (#97), which has no server: host, port and
    /// user are left blank. Machine-local, like the CA path (`clear_machine_local_paths`).
    #[serde(default)]
    file_path: Option<String>,
    // Legacy (read-only, never re-saved)
    #[serde(default, skip_serializing)]
    tls_enabled: bool,
    #[serde(default, skip_serializing)]
    tls_skip_verify: bool,
}

/// How the MySQL connection uses TLS. Stored as the MySQL option names
/// (`DISABLED`, `PREFERRED`, `REQUIRED`, `VERIFY_CA`, `VERIFY_IDENTITY`), which the UI's
/// choices use too.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
#[serde(from = "String", into = "String")]
enum SslMode {
    #[default]
    Disabled,
    /// TLS when the server offers it, plain text when it does not; the certificate is not
    /// checked (as MySQL's `PREFERRED` and libpq's `prefer`). Each client falls back in its
    /// own way: `connect_mysql_pool`, `redis_client::open`, PostgreSQL's `Prefer`.
    Preferred,
    Required,
    VerifyCa,
    VerifyIdentity,
    /// A value this version does not know: a hand edit, or a mode added by a later
    /// version. It still loads, so one bad profile does not break the whole store, but
    /// connecting refuses it instead of silently dropping TLS (#131). The spelling is
    /// kept, so saving the profile (and the sync mirror) writes it back unchanged.
    Unknown(String),
}

impl From<String> for SslMode {
    fn from(name: String) -> Self {
        match name.as_str() {
            "DISABLED" => Self::Disabled,
            "PREFERRED" => Self::Preferred,
            "REQUIRED" => Self::Required,
            "VERIFY_CA" => Self::VerifyCa,
            "VERIFY_IDENTITY" => Self::VerifyIdentity,
            _ => Self::Unknown(name),
        }
    }
}

impl From<SslMode> for String {
    fn from(mode: SslMode) -> Self {
        match mode {
            SslMode::Disabled => "DISABLED".to_owned(),
            SslMode::Preferred => "PREFERRED".to_owned(),
            SslMode::Required => "REQUIRED".to_owned(),
            SslMode::VerifyCa => "VERIFY_CA".to_owned(),
            SslMode::VerifyIdentity => "VERIFY_IDENTITY".to_owned(),
            SslMode::Unknown(name) => name,
        }
    }
}

fn default_ssh_auth_method() -> String {
    "key".to_owned()
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
    /// The kind of server (#132). Absent in profiles written before it, which are MySQL.
    #[serde(default)]
    engine: Engine,
    /// The server to connect to, for every engine. The key stays `mysql` so that a
    /// version that predates `engine` still reads the whole store: it shows every profile
    /// as MySQL, and only the ones that are not fail when opened. Engine-specific settings
    /// go in here as optional fields.
    mysql: MySqlConfig,
    ssh: Option<SshConfig>,
}

/// The kind of server a profile connects to (#132). Stored as `mysql`, `mariadb`, ...
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
#[serde(from = "String", into = "String")]
enum Engine {
    #[default]
    Mysql,
    /// Speaks the MySQL protocol; only the name differs (display, AI prompt).
    Mariadb,
    /// A key-value store (#98): its own connection (`redis_client`) and window.
    Redis,
    /// A database file (#97): its own connection (`sqlite_client`), the query window.
    Sqlite,
    /// Its own connection (`postgres_client`, #96), the query window.
    Postgresql,
    /// An engine this version does not know, such as one a later version added. Its
    /// spelling is kept so that saving does not turn it into MySQL, and nothing connects
    /// to it as MySQL.
    Unknown(String),
}

impl From<String> for Engine {
    fn from(name: String) -> Self {
        match name.as_str() {
            "mysql" => Self::Mysql,
            "mariadb" => Self::Mariadb,
            "redis" => Self::Redis,
            "sqlite" => Self::Sqlite,
            "postgresql" => Self::Postgresql,
            _ => Self::Unknown(name),
        }
    }
}

impl From<Engine> for String {
    fn from(engine: Engine) -> Self {
        match engine {
            Engine::Mysql => "mysql".to_owned(),
            Engine::Mariadb => "mariadb".to_owned(),
            Engine::Redis => "redis".to_owned(),
            Engine::Sqlite => "sqlite".to_owned(),
            Engine::Postgresql => "postgresql".to_owned(),
            Engine::Unknown(name) => name,
        }
    }
}

impl Engine {
    /// The name shown to people and used in the AI prompts.
    fn display_name(&self) -> &str {
        match self {
            Engine::Mysql => "MySQL",
            Engine::Mariadb => "MariaDB",
            Engine::Redis => "Redis",
            Engine::Sqlite => "SQLite",
            Engine::Postgresql => "PostgreSQL",
            Engine::Unknown(name) => name,
        }
    }

    /// How muSQL talks to this engine. Everything that depends on the engine beyond its
    /// name (the window, the connection) goes through here, so an engine is placed once.
    fn protocol(&self) -> Result<Protocol, String> {
        match self {
            Engine::Mysql | Engine::Mariadb => Ok(Protocol::Mysql),
            Engine::Redis => Ok(Protocol::Redis),
            Engine::Sqlite => Ok(Protocol::Sqlite),
            Engine::Postgresql => Ok(Protocol::Postgres),
            Engine::Unknown(name) => Err(unknown_engine_error(name)),
        }
    }

    /// The port the server listens on unless told otherwise. `None` for SQLite, a file.
    /// An engine this version does not know is taken to be MySQL.
    fn default_port(&self) -> Option<u16> {
        match self {
            Engine::Redis => Some(6379),
            Engine::Postgresql => Some(5432),
            Engine::Sqlite => None,
            Engine::Mysql | Engine::Mariadb | Engine::Unknown(_) => Some(3306),
        }
    }

    /// The window that opens a profile of this engine.
    fn window(&self) -> Result<&'static str, String> {
        match self.protocol()? {
            Protocol::Mysql | Protocol::Sqlite | Protocol::Postgres => Ok(WIN_QUERY),
            Protocol::Redis => Ok(WIN_REDIS),
        }
    }

    /// Ok when the MySQL client can connect to this engine.
    fn ensure_mysql_protocol(&self) -> Result<(), String> {
        match self.protocol()? {
            Protocol::Mysql => Ok(()),
            other => Err(format!(
                "This is a {} connection, not a MySQL one.",
                other.name()
            )),
        }
    }
}

/// The ways muSQL talks to a database; several engines can share one (MySQL and MariaDB).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Mysql,
    Redis,
    /// A local file opened in-process (#97): no server, SSH tunnel or credentials.
    Sqlite,
    /// A server reached as MySQL ones are (credentials, SSH tunnel), through its own
    /// client (#96).
    Postgres,
}

impl Protocol {
    /// The name in messages that say a connection is of another kind.
    fn name(self) -> &'static str {
        match self {
            Protocol::Mysql => "MySQL",
            Protocol::Redis => "Redis",
            Protocol::Sqlite => "SQLite",
            Protocol::Postgres => "PostgreSQL",
        }
    }
}

fn unknown_engine_error(name: &str) -> String {
    format!(
        "Unknown connection type \"{name}\". It may have been added by a newer version of muSQL."
    )
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
    // Version of the musql that last wrote the external sync file (#81). Written only into
    // the sync file (see `sanitized_store_json`), never the local `profiles.json` — hence
    // `skip_serializing_if`. Absent when the file predates this feature or is the local
    // store. Used to stop an older musql from clobbering fields a newer one wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    app_version: Option<String>,
    #[serde(default)]
    groups: Vec<ProfileGroup>,
    items: Vec<ConnectionProfile>,
}

// This build's version, the single source of truth (mirrors src-tauri/Cargo.toml). Stamped
// into the sync file on write and compared against an incoming file's stamp to gate writes.
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

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
    /// Connections left out of an import from another app, as "name (kind)" (#99).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    skipped: Vec<String>,
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
    /// The tunnel makes the TLS to the server (#133, `tunnel_tls`); the local hop is plain.
    tls_by_tunnel: bool,
    _listener_task: tokio::task::JoinHandle<()>,
}

impl SshTunnel {
    /// Whether the client must leave TLS to the tunnel (`tunnel` as `connect_target`
    /// returns it): the argument `redis_client::client` takes.
    fn makes_tls(tunnel: &Option<SshTunnel>) -> bool {
        tunnel.as_ref().is_some_and(|t| t.tls_by_tunnel)
    }
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

/// `~/.ssh`, or `None` when no home folder is set.
fn ssh_dir() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()
        .filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".ssh"))
}

fn ssh_known_hosts_path() -> PathBuf {
    ssh_dir()
        .unwrap_or_else(|| PathBuf::from(".ssh"))
        .join("known_hosts")
}

impl russh::client::Handler for SshHandler {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        let host = self.host.clone();
        let port = self.port;
        // Host certificates are never advertised (Config::default() leaves
        // host_key_certificates empty), so a server cannot present one. Reject it anyway:
        // there is no trusted CA list to check a certificate against.
        let key = match server_public_key {
            russh::keys::PublicKeyOrCertificate::PublicKey { key, .. } => Some(key.clone()),
            russh::keys::PublicKeyOrCertificate::Certificate(_) => None,
        };
        async move {
            let Some(key) = key else {
                return Ok(false);
            };
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
    backend: Backend,
    /// Names the server in the AI prompts, which get the connection from here.
    engine: Engine,
    _tunnel: Option<SshTunnel>,
}

/// What the query window's connection is.
#[derive(Clone)]
enum Backend {
    Mysql(Pool),
    /// The database file (#97), with a connection per tab (see `sqlite_client`).
    Sqlite(Arc<sqlite_client::SqliteSession>),
    /// The server and database (#96), with a connection per tab (see `postgres_client`).
    Postgres(Arc<postgres_client::PostgresSession>),
}

/// A statement running for a tab, which `cancel_query` stops.
enum RunningQueryEntry {
    /// `KILL QUERY` on another connection of the pool.
    Mysql { connection_id: u32, pool: Pool },
    /// Interrupted on the tab's own connection.
    Sqlite(Arc<sqlite_client::TabConnection>),
    /// Cancelled through the tab's connection's cancel token.
    Postgres(Arc<postgres_client::TabConnection>),
}

static RUNNING_QUERIES: std::sync::LazyLock<Mutex<HashMap<String, RunningQueryEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// The key a tab's statement is filed under in `RUNNING_QUERIES`. Queries sent without a
/// tab (the table list, the Data view) share one.
fn running_query_key(tab_id: Option<String>) -> String {
    tab_id.unwrap_or_else(|| "__internal__".to_owned())
}

/// Files a running statement in `RUNNING_QUERIES` for `cancel_query`, and takes it off
/// again when dropped, whichever way the statement ends.
struct RunningQueryGuard(String);

impl RunningQueryGuard {
    fn register(key: String, entry: RunningQueryEntry) -> Self {
        if let Ok(mut map) = RUNNING_QUERIES.lock() {
            map.insert(key.clone(), entry);
        }
        Self(key)
    }
}

impl Drop for RunningQueryGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = RUNNING_QUERIES.lock() {
            map.remove(&self.0);
        }
    }
}

static SCHEMA_CACHE: std::sync::LazyLock<Mutex<HashMap<String, SchemaInfo>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

// Window label constants — must match the labels in tauri.conf.json.
/// How long to wait for a server to answer when connecting: the SSH host, and Redis.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

const WIN_MAIN: &str = "main";
const WIN_QUERY: &str = "query";
const WIN_REDIS: &str = "redis";
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
    // A PostgreSQL connection is to one database (#96); MySQL switches with `USE`.
    let database = match request.engine.protocol() {
        Ok(Protocol::Postgres) => request.mysql.database.as_deref().unwrap_or("").trim(),
        _ => "",
    };
    format!(
        "{}:{}:{}:{}:ssl={:?}:ca={}:file={}:db={}|{}",
        String::from(request.engine.clone()),
        request.mysql.host,
        request.mysql.port,
        request.mysql.username,
        request.mysql.ssl_mode,
        request.mysql.tls_ca_cert_path.as_deref().unwrap_or(""),
        request.mysql.sqlite_file(),
        database,
        ssh_part
    )
}

/// The query window's session on `request`'s database file (#97), which replaces any
/// other connection. Nothing is opened here: each tab opens the file on its first query.
fn use_sqlite_file(
    cache: &Mutex<Option<ConnectionCache>>,
    request: &ConnectionRequest,
) -> Result<Arc<sqlite_client::SqliteSession>, String> {
    let fingerprint = connection_fingerprint(request);
    let mut guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
    if let Some(ConnectionCache {
        fingerprint: cached,
        backend: Backend::Sqlite(session),
        ..
    }) = guard.as_ref()
    {
        if *cached == fingerprint {
            return Ok(Arc::clone(session));
        }
    }
    let session = Arc::new(sqlite_client::SqliteSession::new(
        request.mysql.sqlite_file().to_owned(),
    ));
    *guard = Some(ConnectionCache {
        fingerprint,
        backend: Backend::Sqlite(Arc::clone(&session)),
        engine: request.engine.clone(),
        _tunnel: None,
    });
    Ok(session)
}

async fn get_or_create_pool_async(
    cache: &Arc<Mutex<Option<ConnectionCache>>>,
    request: &ConnectionRequest,
) -> Result<Pool, String> {
    request.engine.ensure_mysql_protocol()?;
    let fp = connection_fingerprint(request);

    // Check cache (short lock, no await)
    {
        let guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
        if let Some(ConnectionCache {
            fingerprint,
            backend: Backend::Mysql(pool),
            ..
        }) = guard.as_ref()
        {
            if *fingerprint == fp {
                return Ok(pool.clone());
            }
        }
    }

    // Drop old cache first (frees SSH tunnel port)
    {
        let mut guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
        *guard = None;
    }

    // Set up SSH tunnel if needed (async)
    let (target_host, target_port, tunnel) = connect_target(request).await?;

    // Build MySQL pool (blocking)
    let mut mysql_no_db = request.mysql.clone();
    mysql_no_db.database = None;
    let pool = tauri::async_runtime::spawn_blocking(move || {
        let (pool, _conn) = connect_mysql_pool(&mysql_no_db, &target_host, target_port)?;
        Ok::<Pool, String>(pool)
    })
    .await
    .map_err(|e| format!("Task error: {e}"))??;

    // Update cache
    {
        let mut guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
        *guard = Some(ConnectionCache {
            fingerprint: fp,
            backend: Backend::Mysql(pool.clone()),
            engine: request.engine.clone(),
            _tunnel: tunnel,
        });
    }

    Ok(pool)
}

/// The query window's session on `request`'s PostgreSQL server and database (#96), which
/// replaces any other connection. The credentials are resolved by the caller.
async fn use_postgres_server(
    cache: &Arc<Mutex<Option<ConnectionCache>>>,
    request: &ConnectionRequest,
) -> Result<Arc<postgres_client::PostgresSession>, String> {
    let fingerprint = connection_fingerprint(request);
    {
        let mut guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
        if let Some(ConnectionCache {
            fingerprint: cached,
            backend: Backend::Postgres(session),
            ..
        }) = guard.as_ref()
        {
            if *cached == fingerprint {
                return Ok(Arc::clone(session));
            }
        }
        // Dropped first: the old SSH tunnel frees its port.
        *guard = None;
    }

    let (target_host, target_port, tunnel) = connect_target(request).await?;
    let session = Arc::new(postgres_client::PostgresSession::new(
        &request.mysql,
        &target_host,
        target_port,
    )?);
    // Connects the connection the window's own queries use now, so that a wrong password
    // or an unreachable server fails here, as building the MySQL pool does.
    let opening = Arc::clone(&session);
    tauri::async_runtime::spawn_blocking(move || {
        opening
            .tab(&running_query_key(None), postgres_client::Reopen::Quietly)
            .map(|_| ())
    })
    .await
    .map_err(|e| format!("Task error: {e}"))??;

    let mut guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;
    *guard = Some(ConnectionCache {
        fingerprint,
        backend: Backend::Postgres(Arc::clone(&session)),
        engine: request.engine.clone(),
        _tunnel: tunnel,
    });
    Ok(session)
}

fn build_opts(mysql: &MySqlConfig, host: &str, port: u16) -> Result<OptsBuilder, String> {
    let mut builder = OptsBuilder::new();
    builder = builder
        .ip_or_hostname(Some(host.to_owned()))
        .tcp_port(port)
        .user(Some(mysql.username.clone()))
        .pass(Some(mysql.password.clone()))
        .stmt_cache_size(Some(0));

    if let Some(db) = &mysql.database {
        if !db.trim().is_empty() {
            builder = builder.db_name(Some(db.clone()));
        }
    }

    match &mysql.ssl_mode {
        SslMode::Disabled => {}
        // PREFERRED asks for TLS as REQUIRED does; `connect_mysql_pool` drops it when the
        // server has none.
        SslMode::Preferred | SslMode::Required => {
            builder = builder.ssl_opts(Some(
                SslOpts::default()
                    .with_danger_accept_invalid_certs(true)
                    .with_danger_skip_domain_validation(true),
            ));
        }
        SslMode::VerifyCa => {
            let ssl = SslOpts::default().with_danger_skip_domain_validation(true);
            builder = builder.ssl_opts(Some(
                ssl.with_root_cert_path(mysql.ca_cert_path().map(PathBuf::from)),
            ));
        }
        SslMode::VerifyIdentity => {
            builder = builder.ssl_opts(Some(
                SslOpts::default().with_root_cert_path(mysql.ca_cert_path().map(PathBuf::from)),
            ));
        }
        SslMode::Unknown(name) => return Err(unknown_ssl_mode_error(name)),
    }

    Ok(builder)
}

/// A pool on the MySQL server, and one connection from it, which checks that the server
/// takes the settings. `PREFERRED` tries TLS first and connects without it only when the
/// server says it has none (not on any other failure, such as a wrong password).
fn connect_mysql_pool(
    mysql: &MySqlConfig,
    host: &str,
    port: u16,
) -> Result<(Pool, PooledConn), String> {
    let open = |opts: OptsBuilder| {
        let pool = Pool::new(opts)?;
        let conn = pool.get_conn()?;
        Ok::<_, mysql::Error>((pool, conn))
    };
    let result = match open(build_opts(mysql, host, port)?) {
        Err(mysql::Error::DriverError(mysql::DriverError::TlsNotSupported))
            if mysql.ssl_mode == SslMode::Preferred =>
        {
            let mut plain = mysql.clone();
            plain.ssl_mode = SslMode::Disabled;
            open(build_opts(&plain, host, port)?)
        }
        other => other,
    };
    result.map_err(|e| format!("Failed to connect MySQL: {e}"))
}

fn unknown_ssl_mode_error(name: &str) -> String {
    format!(
        "Unknown SSL mode \"{name}\" in this connection. Choose one in the connection settings."
    )
}

impl MySqlConfig {
    /// The database file of a SQLite connection; blank when none is set.
    fn sqlite_file(&self) -> &str {
        self.file_path.as_deref().map(str::trim).unwrap_or("")
    }

    /// The CA certificate to check the server against; a blank path means none.
    fn ca_cert_path(&self) -> Option<&str> {
        self.tls_ca_cert_path
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
    }
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
            app_version: None,
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
                SslMode::Required
            } else {
                SslMode::VerifyIdentity
            };
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
    write_local_profiles(app, store)?;
    // Mirror to the external sync file (if configured) so every profile change from
    // any window is pushed out automatically (#47). Passwords are keyring-only and are
    // already empty in the store, so no secret is written.
    mirror_to_sync(app, store);
    Ok(())
}

/// Writes the local `profiles.json` only. The sync code uses this while it holds
/// `SYNC_STATE`, because `save_profiles` would mirror and take the lock again.
fn write_local_profiles(app: &AppHandle, store: &ConnectionProfileStore) -> Result<(), String> {
    let path = profiles_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
    }
    let data = serde_json::to_string_pretty(store)
        .map_err(|e| format!("Failed to serialize profiles: {e}"))?;
    std::fs::write(&path, data).map_err(|e| format!("Failed to write profiles: {e}"))
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
            .to_owned(),
        Err(_) => String::new(),
    }
}

// Fields holding a path into the local filesystem. These are meaningless (or wrong) on
// another machine — the same key/certificate (or SQLite file, #97) lives under a different
// user directory, or not at all — so they are excluded from the sync file and never
// overwrite a local value on import (#80). `config_host` is deliberately NOT machine-local: it is an alias into
// the user's own ~/.ssh/config, which is typically kept identical across machines.
fn clear_machine_local_paths(profile: &mut ConnectionProfile) {
    profile.request.mysql.tls_ca_cert_path = None;
    profile.request.mysql.file_path = None;
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
    // Stamp this build's version so another machine can tell who last wrote the file and
    // refuse to overwrite it with an older musql that would drop fields it doesn't know (#81).
    clone.app_version = Some(APP_VERSION.to_owned());
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

// True when the sync file `store` was last written by a strictly newer musql than this
// build (#81). Such a file may carry fields this build doesn't understand; serde would drop
// them on read, so writing our view back would silently strip them for every machine. When
// the file carries no (or an unparseable) version we return false, and a missing or
// malformed file never gets here (`read_sync_for_write` lets the write go ahead), matching
// the pre-#81 behavior for those cases.
fn store_written_by_newer(store: &ConnectionProfileStore) -> bool {
    let Some(other) = store.app_version.as_deref() else {
        return false;
    };
    match (
        semver::Version::parse(other),
        semver::Version::parse(APP_VERSION),
    ) {
        (Ok(other_v), Ok(mine)) => other_v > mine,
        _ => false,
    }
}

/// The sync file as this process last read or wrote it (#143). Comparing against it tells
/// which profiles another machine changed since, so a merge takes those and keeps this
/// machine's version of the rest. Held in memory only; the first import after startup
/// fills it, and `set_sync_path` clears it.
struct SyncBaseline {
    /// The file's text, to skip the merge (and the write) when nothing changed.
    text: String,
    store: ConnectionProfileStore,
}

/// Every read-merge-write of the sync file runs under this lock, which also guards the
/// baseline. Never call `save_profiles` while holding it (its mirror takes the lock).
static SYNC_STATE: Mutex<Option<SyncBaseline>> = Mutex::new(None);

fn sync_state() -> std::sync::MutexGuard<'static, Option<SyncBaseline>> {
    SYNC_STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Takes `SYNC_STATE` for code that then loads the local store under it. The store must be
/// loaded under the lock (a save landing between the load and the lock would be lost), but
/// a migration in `load_profiles` saves, which mirrors and takes the lock. So load once
/// before the lock to run any migration; the caller's load under the lock then has none.
fn load_profiles_for_sync(
    app: &AppHandle,
) -> Result<std::sync::MutexGuard<'static, Option<SyncBaseline>>, String> {
    load_profiles(app)?;
    Ok(sync_state())
}

/// Parses sync-file text with every profile already scrubbed, so the incoming side and the
/// baseline compare like for like.
fn parse_sync_store(text: &str) -> Result<ConnectionProfileStore, String> {
    let mut store: ConnectionProfileStore =
        serde_json::from_str(text).map_err(|e| e.to_string())?;
    store.items.iter_mut().for_each(scrub_incoming_profile);
    Ok(store)
}

fn same_json<T: Serialize>(a: &T, b: &T) -> bool {
    match (serde_json::to_value(a), serde_json::to_value(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Merges one list (groups or profiles) of the sync file into the local one, by id.
/// - An entry that is the same as in `baseline` was not changed by another machine since
///   this process last saw the file, so the local entry stays (and one deleted here stays
///   deleted).
/// - Any other entry is taken from the file; `take` carries over what must stay local.
/// - `prefer_local` (a write, whose `local` holds what the user just saved) keeps the local
///   entry instead when this machine changed it too, or when there is no baseline to tell.
///   Without it (an import), the file wins those, as it did before #143.
///
/// `normalize` turns a local entry into the form the sync file holds, for comparing it
/// with the baseline. Returns whether `local` changed.
fn merge_sync_entries<T: Clone + Serialize>(
    local: &mut Vec<T>,
    incoming: &[T],
    baseline: Option<&[T]>,
    prefer_local: bool,
    id: impl Fn(&T) -> &str,
    normalize: impl Fn(&T) -> T,
    take: impl Fn(&T, &mut T),
) -> bool {
    let mut changed = false;
    for it in incoming {
        let base = baseline.and_then(|b| b.iter().find(|x| id(x) == id(it)));
        if base.is_some_and(|b| same_json(b, it)) {
            continue;
        }
        match local.iter_mut().find(|x| id(x) == id(it)) {
            Some(existing) => {
                // No baseline at all also leaves `base` empty, which counts as changed.
                let local_changed = base.is_none_or(|b| !same_json(b, &normalize(existing)));
                if prefer_local && local_changed {
                    continue;
                }
                let mut new = it.clone();
                take(existing, &mut new);
                if !same_json(existing, &new) {
                    *existing = new;
                    changed = true;
                }
            }
            None => {
                local.push(it.clone());
                changed = true;
            }
        }
    }
    changed
}

/// Merges a parsed (scrubbed) sync file into `local`. Deletions do not propagate. Returns
/// whether `local` changed.
fn merge_sync(
    local: &mut ConnectionProfileStore,
    incoming: &ConnectionProfileStore,
    baseline: Option<&ConnectionProfileStore>,
    prefer_local: bool,
) -> bool {
    let groups = merge_sync_entries(
        &mut local.groups,
        &incoming.groups,
        baseline.map(|b| b.groups.as_slice()),
        prefer_local,
        |g| &g.id,
        Clone::clone,
        |_, _| {},
    );
    let items = merge_sync_entries(
        &mut local.items,
        &incoming.items,
        baseline.map(|b| b.items.as_slice()),
        prefer_local,
        |p| &p.id,
        |p| {
            let mut p = p.clone();
            scrub_incoming_profile(&mut p);
            p
        },
        carry_over_machine_local_paths,
    );
    groups || items
}

/// Reads the sync file before writing it: its text (`None` when there is no file yet) and
/// its parsed store (`None` when malformed, which is overwritten as before #143). Refuses
/// with `SYNC_BLOCKED_NEWER` when a newer muSQL wrote it (#81).
fn read_sync_for_write(
    path: &str,
) -> Result<(Option<String>, Option<ConnectionProfileStore>), String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((None, None)),
        // Don't overwrite what we could not read (a cloud placeholder that failed to
        // download, a locked file): it may hold another machine's changes.
        Err(e) => return Err(format!("Failed to read sync file: {e}")),
    };
    let store = parse_sync_store(&text).ok();
    if store.as_ref().is_some_and(store_written_by_newer) {
        return Err(SYNC_BLOCKED_NEWER.to_owned());
    }
    Ok((Some(text), store))
}

/// Writes `store` to the sync file at `path`. The file is read first: if another machine
/// changed it since `state`'s baseline, those changes are merged into `store` (and the local
/// profiles) before writing, so they are not overwritten. Refuses with `SYNC_BLOCKED_NEWER`
/// when a newer muSQL wrote the file (#81). Caller holds `SYNC_STATE`.
fn push_to_sync_locked(
    app: &AppHandle,
    path: &str,
    state: &mut Option<SyncBaseline>,
    store: &mut ConnectionProfileStore,
) -> Result<(), String> {
    let (current, incoming) = read_sync_for_write(path)?;
    if let Some(incoming) = incoming {
        let baseline = state.as_ref();
        if baseline.is_none_or(|b| Some(&b.text) != current.as_ref())
            && merge_sync(store, &incoming, baseline.map(|b| &b.store), true)
        {
            write_local_profiles(app, store)?;
        }
    }

    let data =
        sanitized_store_json(store).map_err(|e| format!("Failed to serialize sync data: {e}"))?;
    // Rewriting the same content would only make the sync client upload it again.
    if current.as_ref() != Some(&data) {
        sync_folder::write_atomic(std::path::Path::new(path), data.as_bytes())
            .map_err(|e| format!("Failed to write sync file: {e}"))?;
    }
    *state = Some(SyncBaseline {
        store: parse_sync_store(&data)?,
        text: data,
    });
    Ok(())
}

// Best-effort mirror on every save (the sync folder may be temporarily offline; a failure
// here must not fail the profile save).
fn mirror_to_sync(app: &AppHandle, store: &ConnectionProfileStore) {
    // The path is read under the lock, so `set_sync_path` cannot switch it in between.
    let mut state = sync_state();
    let path = read_sync_path(app);
    if path.is_empty() {
        return;
    }
    let mut store = store.clone();
    match push_to_sync_locked(app, &path, &mut state, &mut store) {
        Ok(()) => {}
        // Don't let this build overwrite a file a newer musql wrote — we'd strip fields we
        // can't round-trip (#81). Silently skipping matches the best-effort contract.
        Err(e) if e == SYNC_BLOCKED_NEWER => app_log::warn_throttled(
            app_log::LONG,
            &format!("sync mirror skipped: {path} was written by a newer muSQL"),
        ),
        Err(e) => {
            app_log::warn_throttled(app_log::LONG, &format!("sync mirror to {path} failed: {e}"));
        }
    }
}

// Sentinel returned by the write commands when a newer musql owns the sync file; the UI maps
// it to a localized message. Kept stable and distinct from real error text.
const SYNC_BLOCKED_NEWER: &str = "sync_blocked_newer";

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
    // Under the lock, so a mirror in flight cannot store the old file's baseline after the
    // switch. The baseline describes the old file, so drop it only when the path changes
    // (the sync modal saves the path before every import and export).
    let mut state = sync_state();
    let path = path.trim();
    if read_sync_path(&app) != path {
        *state = None;
    }
    std::fs::write(&file, path).map_err(|e| format!("Failed to write sync path: {e}"))?;
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
    incoming
        .request
        .mysql
        .file_path
        .clone_from(&local.request.mysql.file_path);
    if let Some(new_ssh) = incoming.request.ssh.as_mut() {
        new_ssh.private_key_path = local
            .request
            .ssh
            .as_ref()
            .and_then(|s| s.private_key_path.clone());
    }
}

// Pull the external sync file and merge it into the local store (`merge_sync`: id-based,
// profiles another machine changed win except machine-local paths, local-only entries are
// kept, deletions do not propagate). Conflict copies the sync client left next to the file
// are merged too, then deleted once the merged result is written (#143).
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
    // The sync file is externally writable (a shared cloud folder, possibly hand edited), so
    // `parse_sync_store` scrubs secrets and the transient clear_* commands from every profile.
    let (text, incoming) = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| parse_sync_store(&text).map(|store| (text, store)))
    {
        Ok(read) => read,
        Err(e) => {
            // Runs on every window focus, so an unreachable file would otherwise add a
            // line each time.
            app_log::warn_throttled(
                app_log::LONG,
                &format!("sync import from {path} skipped: {e}"),
            );
            return list_profiles(app);
        }
    };
    // Read before taking the lock (the reads may wait on a cloud download, and every
    // profile save waits on the lock). Only sync files muSQL wrote count: a JSON file that
    // merely has a similar name and no version stamp is left alone. A copy from a newer
    // muSQL is neither merged nor deleted (#81).
    let copies: Vec<(PathBuf, ConnectionProfileStore)> =
        sync_folder::conflict_copies(std::path::Path::new(&path))
            .into_iter()
            .filter_map(|copy| {
                let store = parse_sync_store(&std::fs::read_to_string(&copy).ok()?).ok()?;
                (store.app_version.is_some() && !store_written_by_newer(&store))
                    .then_some((copy, store))
            })
            .collect();

    let mut state = load_profiles_for_sync(&app)?;
    // The path changed while the file was read: the next import reads the new one.
    if read_sync_path(&app) != path {
        drop(state);
        return list_profiles(app);
    }
    let mut store = load_profiles(&app)?;
    // Only persist (and re-mirror) when the merge actually changed something, so the
    // frequent startup/focus refresh doesn't churn disk or bounce the sync file. A file
    // unchanged since the baseline (most focus refreshes) can change nothing.
    let file_changed = state.as_ref().is_none_or(|b| b.text != text);
    let mut changed = false;
    if file_changed || !copies.is_empty() {
        let baseline = state.as_ref().map(|b| &b.store);
        changed = merge_sync(&mut store, &incoming, baseline, false);
        // A copy is a write that lost the race, of unknown age: it may predate what the
        // main file holds now. So it only adds the profiles and groups missing here, and
        // never overrides an existing one (no baseline, local preferred).
        for (_, copy) in &copies {
            changed |= merge_sync(&mut store, copy, None, true);
        }
    }
    if changed {
        write_local_profiles(&app, &store)?;
    }
    // Only now: if the local write failed, the next import must still see the file as new.
    if file_changed {
        *state = Some(SyncBaseline {
            text,
            store: incoming,
        });
    }
    // Write the merged result, and drop the conflict copies only once it is in the file.
    let pushed = (changed || !copies.is_empty())
        .then(|| push_to_sync_locked(&app, &path, &mut state, &mut store));
    match pushed {
        None => {}
        Some(Ok(())) => {
            if !copies.is_empty() {
                log::info!("merged {} sync conflict copies", copies.len());
            }
            for (copy, _) in &copies {
                if let Err(e) = std::fs::remove_file(copy) {
                    log::warn!(
                        "failed to delete sync conflict copy {}: {e}",
                        copy.display()
                    );
                }
            }
        }
        Some(Err(e)) => {
            app_log::warn_throttled(app_log::LONG, &format!("sync mirror to {path} failed: {e}"))
        }
    }
    Ok(ProfileListResponse {
        groups: store.groups,
        items: store.items,
        saved_id: None,
    })
}

// Manually push the current store to the external sync file (surfaces write errors). Like
// the auto-mirror, it first merges what another machine changed since the last read, so the
// UI reloads the list afterwards.
#[tauri::command]
fn sync_export(app: AppHandle) -> Result<bool, String> {
    let mut state = load_profiles_for_sync(&app)?;
    let path = read_sync_path(&app);
    if path.is_empty() {
        return Ok(false);
    }
    let mut store = load_profiles(&app)?;
    // A manual export must not clobber a file a newer musql wrote (#81): unlike the silent
    // auto-mirror, the SYNC_BLOCKED_NEWER error tells the user why it was refused.
    push_to_sync_locked(&app, &path, &mut state, &mut store)?;
    Ok(true)
}

// The synced folders found on this machine (OneDrive, Dropbox), offered as destinations
// in the sync settings (#143).
#[tauri::command]
fn sync_folder_candidates() -> Vec<sync_folder::SyncFolder> {
    sync_folder::candidates()
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
        // The key names the entry (a profile id or a provider), never the secret itself.
        entry
            .set_password(value)
            .map_err(|e| format!("Keyring save error: {e}"))
            .inspect_err(|e| log::warn!("{e} (entry {key})"))?;
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
    let id = profile_id.map(str::to_owned);
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
        AiProvider::Claude => "ai:claude".to_owned(),
        AiProvider::OpenAi => "ai:openai".to_owned(),
        AiProvider::Gemini => "ai:gemini".to_owned(),
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
        return Err("AI API key not configured".to_owned());
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

    let columns = rows
        .into_iter()
        .map(|row| {
            let table_name: String = row.get(0).unwrap_or_default();
            let col = SchemaColumn {
                name: row.get(1).unwrap_or_default(),
                data_type: row.get(2).unwrap_or_default(),
                column_key: row.get(3).unwrap_or_default(),
                is_nullable: row.get(4).unwrap_or_default(),
            };
            (table_name, col)
        })
        .collect();
    Ok(group_schema(database, columns))
}

/// Gathers (table, column) pairs, in column order, into the schema for the AI prompts:
/// the first 100 tables by name.
fn group_schema(database: &str, columns: Vec<(String, SchemaColumn)>) -> SchemaInfo {
    let mut table_map: std::collections::BTreeMap<String, Vec<SchemaColumn>> =
        std::collections::BTreeMap::new();
    for (table_name, col) in columns {
        table_map.entry(table_name).or_default().push(col);
    }
    let tables: Vec<SchemaTable> = table_map
        .into_iter()
        .take(100)
        .map(|(name, columns)| SchemaTable { name, columns })
        .collect();
    SchemaInfo {
        database: database.to_owned(),
        tables,
    }
}

// ── AI prompt building ──

// Upper bound in bytes for the schema text (or the Redis key list) sent to the AI provider.
const AI_CONTEXT_TEXT_MAX: usize = 8000;

fn schema_text_for_prompt(schema: &SchemaInfo) -> String {
    let mut schema_text = String::new();
    for table in &schema.tables {
        let _ = writeln!(schema_text, "-- {}", table.name);
        for col in &table.columns {
            let key_info = match col.column_key.as_str() {
                "PRI" => " PK",
                "MUL" => " FK",
                "UNI" => " UQ",
                _ => "",
            };
            let _ = writeln!(
                schema_text,
                "--   {} {}{}",
                col.name, col.data_type, key_info
            );
        }
    }
    truncate_for_prompt(&mut schema_text, "-- (truncated)");
    schema_text
}

/// Cuts `text` to `AI_CONTEXT_TEXT_MAX` bytes, on a whole line, and marks the cut.
fn truncate_for_prompt(text: &mut String, marker: &str) {
    if text.len() <= AI_CONTEXT_TEXT_MAX {
        return;
    }
    // String::truncate panics off a char boundary, and identifiers can be multibyte.
    let mut end = AI_CONTEXT_TEXT_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    // Prefer ending on a whole line so the model never sees a half identifier.
    if let Some(nl) = text[..end].rfind('\n') {
        end = nl + 1;
    }
    text.truncate(end);
    let _ = writeln!(text, "\n{marker}");
}

fn build_ai_prompt(
    engine: &Engine,
    schema: &SchemaInfo,
    text_before: &str,
    text_after: &str,
) -> String {
    let schema_text = schema_text_for_prompt(schema);

    format!(
        "You are a {} query assistant. Given the database schema below and the SQL context, \
         return ONLY the SQL fragment to insert at [CURSOR]. No explanation, no markdown, no code fences.\n\n\
         Database: {}\n\n\
         Schema:\n{}\n\
         SQL context:\n{}\n[CURSOR]\n{}",
        engine.display_name(),
        schema.database,
        schema_text,
        text_before,
        text_after
    )
}

// ── AI API call ──

/// Sends `prompt` to the provider. A failure is logged here, once for every caller. The
/// error is the provider's status and message; the key travels in a header and the
/// prompt is never echoed into it.
async fn call_ai_api(
    provider: &AiProvider,
    model: &str,
    api_key: &str,
    prompt: &str,
) -> Result<String, String> {
    send_ai_request(provider, model, api_key, prompt)
        .await
        .inspect_err(|e| {
            // Throttled: completion fires per keystroke, and a 401 or 429 repeats each time.
            app_log::warn_throttled(app_log::BRIEF, &format!("AI request failed: {e}"))
        })
}

async fn send_ai_request(
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
                .map(|s| s.to_owned())
                .ok_or_else(|| "Claude API: no text in response".to_owned())
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
                .map(|s| s.to_owned())
                .ok_or_else(|| "OpenAI API: no content in response".to_owned())
        }
        AiProvider::Gemini => {
            // The key goes in a header, not the query string, so it cannot end up in a URL
            // that reqwest errors, proxies or logs echo back.
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                model
            );
            let body = serde_json::json!({
                "contents": [{"parts": [{"text": prompt}]}]
            });
            let resp = client
                .post(&url)
                .header("x-goog-api-key", api_key)
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .await
                .map_err(|e| format!("Gemini API request failed: {e}"))?;
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
                .map(|s| s.to_owned())
                .ok_or_else(|| "Gemini API: no text in response".to_owned())
        }
    }
}

// ── AI Tauri commands ──

/// The open connection's engine and the schema of `database`, for the AI prompts. The
/// schema is fetched once per connection and database, then cached.
async fn connection_schema(
    state: &Mutex<Option<ConnectionCache>>,
    database: &str,
) -> Result<(Engine, SchemaInfo), String> {
    let (backend, fingerprint, engine) = {
        let guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
        match guard.as_ref() {
            Some(cached) => (
                cached.backend.clone(),
                cached.fingerprint.clone(),
                cached.engine.clone(),
            ),
            None => return Err("Not connected".to_owned()),
        }
    };

    let cache_key = format!("{fingerprint}:{database}");

    let cached = {
        let cache = SCHEMA_CACHE
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        cache.get(&cache_key).cloned()
    };
    if let Some(schema) = cached {
        return Ok((engine, schema));
    }

    let db = database.to_owned();
    let schema = tauri::async_runtime::spawn_blocking(move || match backend {
        Backend::Mysql(pool) => fetch_schema(&pool, &db),
        Backend::Sqlite(session) => sqlite_client::schema(session.path(), &db),
        Backend::Postgres(session) => session
            .tab(&running_query_key(None), postgres_client::Reopen::Quietly)?
            .lock()
            .schema(&db),
    })
    .await
    .map_err(|e| format!("Task error: {e}"))??;
    if let Ok(mut cache) = SCHEMA_CACHE.lock() {
        cache.insert(cache_key, schema.clone());
    }
    Ok((engine, schema))
}

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
    let (engine, schema) = connection_schema(&state, &database).await?;
    let prompt = build_ai_prompt(&engine, &schema, &text_before, &text_after);
    let result = ask_ai(&provider, &model, op_ref, &prompt).await?;
    Ok(result.trim().to_owned())
}

/// A provider name from the UI (`claude`, `openai`, `gemini`).
fn parse_ai_provider(provider: &str) -> Result<AiProvider, String> {
    serde_json::from_value(serde_json::json!(provider))
        .map_err(|_| format!("Invalid AI provider: {provider}"))
}

/// Sends `prompt` to the provider the UI named, with its key (`resolve_ai_api_key`).
async fn ask_ai(
    provider: &str,
    model: &str,
    op_ref: Option<String>,
    prompt: &str,
) -> Result<String, String> {
    let ai_provider = parse_ai_provider(provider)?;
    let api_key = resolve_ai_api_key(&ai_provider, op_ref).await?;
    call_ai_api(&ai_provider, model, &api_key, prompt).await
}

fn build_ai_assist_prompt(
    engine: &Engine,
    schema: &SchemaInfo,
    prompt: &str,
    editor_content: &str,
    conversation_context: &str,
) -> String {
    let schema_text = schema_text_for_prompt(schema);

    let mut parts = format!(
        "You are a {} query assistant. Given the database schema below, \
         write SQL based on the user's request. Return ONLY the SQL query. \
         No explanation, no markdown, no code fences.\n\n\
         Database: {}\n\n\
         Schema:\n{}",
        engine.display_name(),
        schema.database,
        schema_text
    );

    if !editor_content.trim().is_empty() {
        let _ = writeln!(parts, "\nCurrent SQL in editor:\n{}", editor_content);
    }
    if !conversation_context.trim().is_empty() {
        let _ = writeln!(parts, "\nPrevious conversation:\n{}", conversation_context);
    }
    let _ = write!(parts, "\nUser request: {}", prompt);
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
    let (engine, schema) = connection_schema(&state, &database).await?;
    let ai_prompt = build_ai_assist_prompt(
        &engine,
        &schema,
        &prompt,
        &editor_content,
        &conversation_context,
    );
    let result = ask_ai(&provider, &model, op_ref, &ai_prompt).await?;
    Ok(strip_code_fence(&result))
}

/// The reply without the code fence a model may wrap it in despite being asked not to
/// (```` ```sql ````, ```` ```redis ````, or a bare ```` ``` ````).
fn strip_code_fence(reply: &str) -> String {
    let trimmed = reply.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed.to_owned();
    };
    let body = match rest.split_once('\n') {
        // The opening fence's line, language tag and all, goes.
        Some((_, body)) => body,
        // All on one line (```sql SELECT 1```): only a known tag goes, as a command's
        // first word cannot be told from a tag otherwise.
        None => ["sql", "SQL", "redis", "bash", "sh", "shell", "text"]
            .iter()
            .find_map(|tag| rest.strip_prefix(tag).filter(|r| r.starts_with(' ')))
            .unwrap_or(rest),
    };
    body.trim_end().trim_end_matches("```").trim().to_owned()
}

fn build_redis_ai_prompt(
    version: &str,
    db: i64,
    mut keys: String,
    prompt: &str,
    conversation_context: &str,
) -> String {
    truncate_for_prompt(&mut keys, "(truncated)");
    let mut parts = format!(
        "You are a Redis assistant. Write redis-cli commands for the user's request. \
         Return ONLY the commands, one per line, in redis-cli syntax (put an argument with \
         spaces in double quotes). No explanation, no markdown, no code fences. Use SCAN \
         rather than KEYS.\n\n\
         Redis version: {version}\n\
         Current database: {db}\n\n\
         Some of the keys, with their types (values are not shown):\n{keys}"
    );
    if !conversation_context.trim().is_empty() {
        let _ = writeln!(parts, "\nPrevious conversation:\n{conversation_context}");
    }
    let _ = write!(parts, "\nUser request: {prompt}");
    parts
}

/// The AI assist of the Redis window (#98): redis-cli commands for `prompt`. The prompt
/// carries key names and types (`redis_client::ai_context`), not values.
#[tauri::command]
async fn redis_ai_assist(
    prompt: String,
    conversation_context: String,
    provider: String,
    model: String,
    op_ref: Option<String>,
    state: tauri::State<'_, redis_client::RedisState>,
) -> Result<String, String> {
    let connection = redis_client::current_epoch();
    let (version, db, keys) = with_redis_session(&state, move |s| {
        redis_client::ai_context(s, || redis_client::is_current(connection))
    })
    .await?;
    let ai_prompt = build_redis_ai_prompt(&version, db, keys, &prompt, &conversation_context);
    let result = ask_ai(&provider, &model, op_ref, &ai_prompt).await?;
    Ok(strip_code_fence(&result))
}

#[tauri::command]
fn save_ai_api_key(provider: String, api_key: String) -> Result<(), String> {
    set_ai_api_key(&parse_ai_provider(&provider)?, &api_key)
}

#[tauri::command]
fn has_ai_api_key(provider: String) -> bool {
    parse_ai_provider(&provider).is_ok_and(|p| !get_ai_api_key(&p).is_empty())
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
                    hosts.push(pattern.to_owned());
                }
            }
        }
    }
    hosts
}

#[tauri::command]
fn list_ssh_config_hosts() -> Vec<String> {
    let Some(dir) = ssh_dir() else {
        return vec![];
    };
    let path = dir.join("config");
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
            hostname = Some(value.to_owned());
        } else if lower.starts_with("port") && port.is_none() {
            port = value.parse().ok();
        } else if lower.starts_with("user")
            && !lower.starts_with("userknownhostsfile")
            && user.is_none()
        {
            user = Some(value.to_owned());
        } else if lower.starts_with("identityfile") && identity_file.is_none() {
            let expanded = if value.starts_with("~/") || value == "~" {
                value.replacen('~', home, 1)
            } else {
                value.to_owned()
            };
            identity_file = Some(expanded);
        }
    }

    (
        hostname.unwrap_or_else(|| alias.to_owned()),
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
        return (alias.to_owned(), 22, None, None);
    }
    let path = std::path::Path::new(&home).join(".ssh").join("config");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return (alias.to_owned(), 22, None, None),
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
            path.to_owned()
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
        if let Some(ssh_dir) = ssh_dir() {
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
    "SSH authentication failed: no valid key found. Specify a private key path in SSH settings.".to_owned(),
  )
}

/// Where to connect for `request`: its server, or the local end of an SSH tunnel to it.
/// The tunnel stays open while the returned `SshTunnel` is kept.
async fn connect_target(
    request: &ConnectionRequest,
) -> Result<(String, u16, Option<SshTunnel>), String> {
    match &request.ssh {
        Some(ssh) if ssh.enabled => {
            let tls = tunnel_tls(request)?;
            let tunnel =
                start_ssh_tunnel(ssh, &request.mysql.host, request.mysql.port, tls).await?;
            Ok(("127.0.0.1".to_owned(), tunnel.local_port, Some(tunnel)))
        }
        _ => Ok((request.mysql.host.clone(), request.mysql.port, None)),
    }
}

/// The TLS the SSH tunnel makes itself, for a `VERIFY_IDENTITY` connection through it
/// (#133). The client reaches the tunnel's end at 127.0.0.1, so its own TLS would check
/// the certificate against that address instead of the server's name. Redis's TLS starts
/// with the connection, so the tunnel makes it (`redis_client::tunnel_tls`). MySQL's starts
/// inside its handshake, which the tunnel cannot step into, and the mysql crate has no
/// way to check another name than the one it connects to: refused, with VERIFY_CA named.
fn tunnel_tls(request: &ConnectionRequest) -> Result<Option<redis_client::TunnelTls>, String> {
    // A server on the jump host itself, named 127.0.0.1: the client's check is right.
    if request.mysql.ssl_mode != SslMode::VerifyIdentity || request.mysql.host.trim() == "127.0.0.1"
    {
        return Ok(None);
    }
    match request.engine.protocol()? {
        Protocol::Mysql => Err(
            "VERIFY_IDENTITY cannot check the server's host name through an SSH tunnel. \
             Use VERIFY_CA, which checks the certificate but not the host name."
                .to_owned(),
        ),
        Protocol::Redis => redis_client::tunnel_tls(&request.mysql).map(Some),
        // A local file: never reaches the tunnel.
        Protocol::Sqlite => Ok(None),
        // The client connects to the tunnel but checks the profile's host (`hostaddr`,
        // see `postgres_client::config`), so its own TLS is right.
        Protocol::Postgres => Ok(None),
    }
}

async fn start_ssh_tunnel(
    ssh: &SshConfig,
    mysql_host: &str,
    mysql_port: u16,
    tls: Option<redis_client::TunnelTls>,
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
        CONNECT_TIMEOUT,
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
        return Err("SSH authentication failed".to_owned());
    }

    // Create local TCP listener on a free port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("Failed to bind local port: {e}"))?;
    let local_port = listener
        .local_addr()
        .map_err(|e| format!("Failed to read local addr: {e}"))?
        .port();

    // TLS made here (#133): one handshake now, so a certificate that does not name the host
    // fails the connection with that reason, not as a connection closed later.
    let tls = tls.map(|t| (tokio_rustls::TlsConnector::from(t.config), t.server_name));
    if let Some((connector, server_name)) = &tls {
        let check = async {
            let channel = session
                .channel_open_direct_tcpip(mysql_host, mysql_port as u32, "127.0.0.1", 0u32)
                .await
                .map_err(|e| {
                    format!("SSH tunnel could not reach {mysql_host}:{mysql_port}: {e}")
                })?;
            connector
                .connect(server_name.clone(), channel.into_stream())
                .await
                .map_err(|e| format!("TLS through the SSH tunnel failed: {e}"))
        };
        // The jump host may take the OS's TCP timeout to report an unreachable server.
        tokio::time::timeout(CONNECT_TIMEOUT, check)
            .await
            .map_err(|_| "TLS through the SSH tunnel timed out".to_owned())??;
    }
    let tls_by_tunnel = tls.is_some();

    // Spawn forwarding task
    let mysql_host = mysql_host.to_owned();
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
            let tls = tls.clone();
            tokio::spawn(async move {
                match tls {
                    None => {
                        let _ =
                            tokio::io::copy_bidirectional(&mut tcp_stream, &mut ssh_stream).await;
                    }
                    // A failed handshake closes the local connection: the client sees it
                    // fail (the check above has already named the reason once). Bounded,
                    // as nothing here watches the client give up meanwhile.
                    Some((connector, server_name)) => {
                        let handshake = connector.connect(server_name, ssh_stream);
                        if let Ok(Ok(mut tls_stream)) =
                            tokio::time::timeout(CONNECT_TIMEOUT, handshake).await
                        {
                            let _ = tokio::io::copy_bidirectional(&mut tcp_stream, &mut tls_stream)
                                .await;
                        }
                    }
                }
            });
        }
    });

    Ok(SshTunnel {
        local_port,
        tls_by_tunnel,
        _listener_task: task,
    })
}

fn mysql_value_to_json(value: Value) -> serde_json::Value {
    match value {
        Value::NULL => serde_json::Value::Null,
        Value::Bytes(v) => bytes_to_json(&v),
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

/// Bytes as the result tables show them: as text when they are UTF-8, else as hex.
fn bytes_to_json(bytes: &[u8]) -> serde_json::Value {
    match std::str::from_utf8(bytes) {
        Ok(s) => json!(s),
        Err(_) => json!(format!("0x{}", hex::encode(bytes))),
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
    request: ConnectionRequest,
    profile_id: Option<String>,
) -> Result<String, String> {
    run_connection_test(request, profile_id)
        .await
        .inspect_err(|e| log::warn!("connection test failed: {e}"))
}

async fn run_connection_test(
    mut request: ConnectionRequest,
    profile_id: Option<String>,
) -> Result<String, String> {
    // Before the credentials: resolving them can run the 1Password CLI.
    let protocol = request.engine.protocol()?;
    let (target_host, target_port, tunnel) = if protocol == Protocol::Sqlite {
        // A local file: no credentials, and no server to tunnel to.
        (String::new(), 0, None)
    } else {
        resolve_credentials(&mut request, profile_id.as_deref()).await?;
        // Set up SSH tunnel if needed (async)
        connect_target(&request).await?
    };
    let tls_by_tunnel = SshTunnel::makes_tls(&tunnel);

    // The connection test itself (blocking). The tunnel stays open until it ends.
    let mysql = request.mysql.clone();
    tauri::async_runtime::spawn_blocking(move || {
        match protocol {
            Protocol::Mysql => {
                let (_pool, mut conn) = connect_mysql_pool(&mysql, &target_host, target_port)?;
                conn.query_drop("SELECT 1")
                    .map_err(|e| format!("MySQL ping failed: {e}"))?;
            }
            Protocol::Redis => {
                redis_client::ping(&mysql, &target_host, target_port, tls_by_tunnel)?;
            }
            Protocol::Sqlite => sqlite_client::ping(mysql.sqlite_file())?,
            Protocol::Postgres => postgres_client::ping(&mysql, &target_host, target_port)?,
        }
        Ok("Connection succeeded.".to_owned())
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
        return Err("Query is empty".to_owned());
    }
    let limit = max_rows.unwrap_or(500);
    let cache = Arc::clone(&state);
    let tab_key = running_query_key(tab_id);

    match request.engine.protocol()? {
        Protocol::Sqlite => {
            let session = use_sqlite_file(&cache, &request)?;
            return tauri::async_runtime::spawn_blocking(move || {
                run_sqlite_query(&session, &query, limit, tab_key)
            })
            .await
            .map_err(|e| format!("Task error: {e}"))?;
        }
        Protocol::Postgres => {
            resolve_credentials(&mut request, profile_id.as_deref()).await?;
            // Safe to log: this fails before any statement runs, so `e` cannot contain SQL.
            let session = use_postgres_server(&cache, &request)
                .await
                .inspect_err(|e| {
                    app_log::warn_throttled(app_log::BRIEF, &format!("connection failed: {e}"))
                })?;
            return tauri::async_runtime::spawn_blocking(move || {
                run_postgres_query(&session, &query, limit, tab_key)
            })
            .await
            .map_err(|e| format!("Task error: {e}"))?;
        }
        Protocol::Mysql | Protocol::Redis => {}
    }

    // Before the credentials: resolving them can run the 1Password CLI (#132).
    request.engine.ensure_mysql_protocol()?;
    resolve_credentials(&mut request, profile_id.as_deref()).await?;

    // Get or create pool (async — SSH tunnel creation is async)
    // Safe to log: this fails before any statement runs, so `e` cannot contain SQL.
    // Errors from the statement itself (below) are never logged.
    let pool = get_or_create_pool_async(&cache, &request)
        .await
        .inspect_err(|e| {
            // Throttled: a dropped connection fails every query of a tab at once.
            app_log::warn_throttled(app_log::BRIEF, &format!("connection failed: {e}"))
        })?;

    let db = request.mysql.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut conn = pool
            .get_conn()
            .map_err(|e| format!("Failed to get connection: {e}"))?;

        // Store connection ID + pool for cancel support (per tab)
        let _running = RunningQueryGuard::register(
            tab_key,
            RunningQueryEntry::Mysql {
                connection_id: conn.connection_id(),
                pool: pool.clone(),
            },
        );

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
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

/// Runs one statement on the connection of the tab `tab_key`, which `cancel_query` can
/// interrupt meanwhile.
fn run_sqlite_query(
    session: &sqlite_client::SqliteSession,
    query: &str,
    limit: usize,
    tab_key: String,
) -> Result<QueryResult, String> {
    // Safe to log: this fails before any statement runs, so `e` cannot contain SQL.
    let tab = session.tab(&tab_key).inspect_err(|e| {
        app_log::warn_throttled(app_log::BRIEF, &format!("connection failed: {e}"))
    })?;
    let conn = tab.lock();
    let _running =
        RunningQueryGuard::register(tab_key, RunningQueryEntry::Sqlite(Arc::clone(&tab)));
    sqlite_client::run(&conn, query, limit)
}

/// Runs one statement on the connection of the tab `tab_key`, which `cancel_query` can
/// cancel meanwhile.
fn run_postgres_query(
    session: &postgres_client::PostgresSession,
    query: &str,
    limit: usize,
    tab_key: String,
) -> Result<QueryResult, String> {
    // The window's own queries (no tab) keep no state, so a connection the server closed
    // is opened again quietly; a SQL tab's may have held a transaction, so the tab is told.
    let reopen = if tab_key == running_query_key(None) {
        postgres_client::Reopen::Quietly
    } else {
        postgres_client::Reopen::AfterTelling
    };
    // Safe to log: this fails before any statement runs, so `e` cannot contain SQL.
    let tab = session.tab(&tab_key, reopen).inspect_err(|e| {
        app_log::warn_throttled(app_log::BRIEF, &format!("connection failed: {e}"))
    })?;
    // Filed once the connection is this statement's: the window's own queries share a
    // connection (and a key), and one waiting its turn must not take the entry of the one
    // running, or drop it when it ends.
    let mut conn = tab.lock();
    let _running =
        RunningQueryGuard::register(tab_key, RunningQueryEntry::Postgres(Arc::clone(&tab)));
    conn.run(query, limit)
}

#[tauri::command]
async fn cancel_query(tab_id: Option<String>) -> Result<(), String> {
    let tab_key = running_query_key(tab_id);
    // What cancels the statement; it reaches the server, so it runs off the lock.
    let cancel: Box<dyn FnOnce() -> Result<(), String> + Send> = {
        let map = RUNNING_QUERIES
            .lock()
            .map_err(|e| format!("Lock error: {e}"))?;
        match map.get(&tab_key) {
            Some(RunningQueryEntry::Mysql {
                connection_id,
                pool,
            }) => {
                let (cid, pool) = (*connection_id, pool.clone());
                if cid == 0 {
                    return Ok(());
                }
                Box::new(move || {
                    let mut conn = pool
                        .get_conn()
                        .map_err(|e| format!("Failed to get cancel connection: {e}"))?;
                    conn.query_drop(format!("KILL QUERY {cid}"))
                        .map_err(|e| format!("Failed to cancel query: {e}"))
                })
            }
            Some(RunningQueryEntry::Sqlite(tab)) => {
                tab.interrupt();
                return Ok(());
            }
            Some(RunningQueryEntry::Postgres(tab)) => {
                let tab = Arc::clone(tab);
                Box::new(move || tab.cancel())
            }
            None => return Ok(()),
        }
    };

    tauri::async_runtime::spawn_blocking(cancel)
        .await
        .map_err(|e| format!("Task error: {e}"))?
}

/// Closes the connection a SQL tab kept (SQLite and PostgreSQL, which keep one per tab);
/// the tab was closed. A tab id is used again by a later tab, which then opens its own.
#[tauri::command]
fn close_query_tab(
    tab_id: String,
    state: tauri::State<'_, Arc<Mutex<Option<ConnectionCache>>>>,
) -> Result<(), String> {
    let guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
    match guard.as_ref().map(|cached| &cached.backend) {
        Some(Backend::Sqlite(session)) => session.close(&tab_id),
        Some(Backend::Postgres(session)) => session.close(&tab_id),
        Some(Backend::Mysql(_)) | None => {}
    }
    Ok(())
}

#[tauri::command]
fn disconnect_pool(
    state: tauri::State<'_, Arc<Mutex<Option<ConnectionCache>>>>,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
    *guard = None;
    Ok(())
}

// ── Redis window commands (#98) ──

#[derive(Serialize)]
struct RedisServer {
    version: String,
    db: i64,
}

/// Opens the Redis window's connection, replacing any earlier one. Credentials and the
/// SSH tunnel are resolved as for MySQL.
#[tauri::command]
async fn redis_connect(
    mut request: ConnectionRequest,
    profile_id: Option<String>,
    state: tauri::State<'_, redis_client::RedisState>,
) -> Result<RedisServer, String> {
    if request.engine.protocol()? != Protocol::Redis {
        return Err("This is not a Redis connection.".to_owned());
    }
    // Drop the earlier session first, which also closes its SSH tunnel. The epoch keeps a
    // connect still running from an earlier open from storing its session after this one.
    let epoch = drop_redis_session(&state).await?;
    resolve_credentials(&mut request, profile_id.as_deref()).await?;
    let (host, port, tunnel) = connect_target(&request).await?;
    let tls_by_tunnel = SshTunnel::makes_tls(&tunnel);
    let mysql = request.mysql.clone();
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (client, conn) = redis_client::open(&mysql, &host, port, tls_by_tunnel)?;
        let mut session = redis_client::RedisSession::new(client, conn, tunnel);
        let version = redis_client::server_version(&mut session)?;
        let db = session.db();
        let mut guard = redis_client::lock(&state);
        if !redis_client::is_current(epoch) {
            // Closed, or opened again, while connecting: the session is dropped here.
            return Err("The connection was cancelled.".to_owned());
        }
        *guard = Some(session);
        Ok(RedisServer { version, db })
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
    .inspect_err(|e: &String| log::warn!("Redis connection failed: {e}"))
}

#[tauri::command]
async fn redis_disconnect(state: tauri::State<'_, redis_client::RedisState>) -> Result<(), String> {
    drop_redis_session(&state).await.map(|_| ())
}

/// Closes the Redis session and starts a new epoch, so a connect still running does not
/// store its session afterwards. Returns the new epoch. The lock is taken off the async
/// runtime: a command still running holds it until the server answers (up to the read
/// timeout). A drop that gets the lock only after a newer connect stored its session
/// leaves that session alone (waiters on the lock are not served in order).
async fn drop_redis_session(state: &redis_client::RedisState) -> Result<u64, String> {
    let epoch = redis_client::next_epoch();
    let state = state.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = redis_client::lock(&state);
        if redis_client::is_current(epoch) {
            *guard = None;
        }
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?;
    Ok(epoch)
}

/// Runs `f` on the open Redis session off the async runtime (the redis calls block).
/// Nothing here is logged: the errors can carry keys and values.
async fn with_redis_session<T: Send + 'static>(
    state: &redis_client::RedisState,
    f: impl FnOnce(&mut redis_client::RedisSession) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let state = state.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut guard = redis_client::lock(&state);
        let session = guard.as_mut().ok_or("Not connected")?;
        f(session)
    })
    .await
    .map_err(|e| format!("Task error: {e}"))?
}

#[tauri::command]
async fn redis_scan(
    cursor: String,
    pattern: String,
    state: tauri::State<'_, redis_client::RedisState>,
) -> Result<redis_client::ScanPage, String> {
    // A string, as `ScanPage` sends it (u64 cursors do not fit a JS number).
    let cursor: u64 = cursor
        .parse()
        .map_err(|_| "Invalid scan cursor.".to_owned())?;
    // Numbered before waiting for the connection, so a listing already running sees that
    // this one superseded it. A disconnect or another connect stops it too (the window
    // was closed or reopened).
    let epoch = redis_client::next_scan_epoch();
    let connection = redis_client::current_epoch();
    with_redis_session(&state, move |s| {
        redis_client::scan(s, cursor, &pattern, || {
            redis_client::is_current_scan(epoch) && redis_client::is_current(connection)
        })
    })
    .await
}

#[tauri::command]
async fn redis_key(
    key: String,
    raw: Option<String>,
    state: tauri::State<'_, redis_client::RedisState>,
) -> Result<redis_client::KeyView, String> {
    // `raw`: the key's bytes in hex, for a key that is not valid UTF-8 (`KeyEntry`).
    let key = redis_client::key_bytes(&key, raw.as_deref())?;
    with_redis_session(&state, move |s| redis_client::key_view(s, &key)).await
}

/// The DB switcher. A failure (a DB number the server does not have) comes back as `Err`.
#[tauri::command]
async fn redis_select(
    db: i64,
    state: tauri::State<'_, redis_client::RedisState>,
) -> Result<(), String> {
    with_redis_session(&state, move |s| s.select(db)).await
}

#[tauri::command]
async fn redis_databases(
    state: tauri::State<'_, redis_client::RedisState>,
) -> Result<Vec<redis_client::DbInfo>, String> {
    with_redis_session(&state, redis_client::databases).await
}

#[tauri::command]
async fn redis_command_docs(
    state: tauri::State<'_, redis_client::RedisState>,
) -> Result<Vec<redis_client::CommandDoc>, String> {
    with_redis_session(&state, redis_client::command_docs).await
}

/// Runs one CLI line. A dangerous command comes back unrun with `confirm` set until it is
/// sent again with `confirmed`.
#[tauri::command]
async fn redis_command(
    line: String,
    confirmed: bool,
    state: tauri::State<'_, redis_client::RedisState>,
) -> Result<redis_client::CommandReply, String> {
    with_redis_session(&state, move |s| redis_client::command(s, &line, confirmed)).await
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
    let (import_path, import_data): (String, ExportData) = if let Some(ref path) = file_path {
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
    merge_import(&app, import_data, mode.as_deref(), Some(import_path), false).map(Some)
}

/// Carries over, when another app's connection overwrites a profile, the settings that
/// app has no notion of: how the passwords are kept (the keyring may hold them, which
/// the imported "ask every time" would leave unused) and the 1Password references.
/// The profile is synced, so dropping them would drop them on every machine.
fn keep_local_settings(existing: &ConnectionRequest, imported: &mut ConnectionRequest) {
    imported.mysql.save_password = existing.mysql.save_password;
    imported.mysql.op_ref = existing.mysql.op_ref.clone();
    if let (Some(old), Some(new)) = (&existing.ssh, &mut imported.ssh) {
        new.save_ssh_password = old.save_ssh_password;
        new.save_ssh_passphrase = old.save_ssh_passphrase;
        new.op_passphrase_ref = old.op_passphrase_ref.clone();
        new.op_password_ref = old.op_password_ref.clone();
    }
}

/// Imports the connections TablePlus keeps (#99), as `import_profiles` does a file: the
/// first call (`mode` None) reports duplicates instead of importing, and the second reads
/// TablePlus again with the mode chosen.
#[tauri::command]
async fn import_tableplus(app: AppHandle, mode: Option<String>) -> Result<ImportResult, String> {
    let imported =
        tauri::async_runtime::spawn_blocking(|| tableplus::read(&tableplus::data_dir()?))
            .await
            .map_err(|e| format!("Task error: {e}"))??;
    let mut result = merge_import(&app, imported.data, mode.as_deref(), None, true)?;
    result.skipped = imported.skipped;
    Ok(result)
}

/// Adds the profiles of an import to the store. With `mode` None it first looks for
/// groups and profiles that are already there, and returns those without importing
/// (`import_path` goes back with them, for the second call); "overwrite" replaces them,
/// "add" (also None without duplicates) adds everything as new. `keep_local` is for a
/// source that does not know muSQL's own settings (`keep_local_settings`).
fn merge_import(
    app: &AppHandle,
    import_data: ExportData,
    mode: Option<&str>,
    import_path: Option<String>,
    keep_local: bool,
) -> Result<ImportResult, String> {
    let store = load_profiles(app)?;

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
            return Ok(ImportResult {
                groups: store.groups,
                items: store.items,
                imported_count: 0,
                conflicts: Some(ImportConflicts {
                    groups: dup_groups,
                    profiles: dup_profiles,
                }),
                file_path: import_path,
                skipped: Vec::new(),
            });
        }
        // No conflicts — fall through to "add" mode
    }

    let effective_mode = mode.unwrap_or("add");

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
                let mut request = item.request.clone();
                if keep_local {
                    keep_local_settings(&existing.request, &mut request);
                }
                // As `save_profile` does: a SQLite profile keeps no credentials.
                if request.engine.protocol() == Ok(Protocol::Sqlite) {
                    delete_password(&existing.id);
                    delete_ssh_passphrase(&existing.id);
                    delete_ssh_password(&existing.id);
                }
                existing.request = request;
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

    save_profiles(app, &store)?;

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

    Ok(ImportResult {
        groups: store.groups,
        items: store.items,
        imported_count,
        conflicts: None,
        file_path: None,
        skipped: Vec::new(),
    })
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
        return Err("Profile name is empty".to_owned());
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
    // A SQLite profile has no credentials (#97): drop those it kept from a server it was
    // switched from, so switching back does not bring them back unasked.
    if profile.request.engine.protocol() == Ok(Protocol::Sqlite) {
        delete_password(&profile.id);
        delete_ssh_passphrase(&profile.id);
        delete_ssh_password(&profile.id);
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
        return Err("Group name is empty".to_owned());
    }
    let mut store = load_profiles(&app)?;
    if let Some(existing_id) = id {
        // Rename existing group
        if let Some(group) = store.groups.iter_mut().find(|g| g.id == existing_id) {
            group.name = name;
        } else {
            return Err("Group not found".to_owned());
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

/// Opens a profile in the window for its engine (#132); a profile of an unknown engine
/// opens none.
#[tauri::command]
fn open_connection_window(app: AppHandle, id: String) -> Result<(), String> {
    let store = load_profiles(&app)?;
    let profile = store
        .items
        .iter()
        .find(|p| p.id == id)
        .ok_or("Profile not found")?;
    let label = profile.request.engine.window()?;
    show_connection_window(&app, label, "open", id)
}

/// Sends `<label>:<event>` (`query:open`, `redis:open`, `query:docker-open`), which only
/// that window's page listens for, then shows the window in place of the main one. The
/// other connection window, if open (a profile opened from the settings window), is
/// closed first: one connection per instance.
fn show_connection_window(
    app: &AppHandle,
    label: &str,
    event: &str,
    payload: impl Serialize + Clone,
) -> Result<(), String> {
    for other in CONNECTION_WINDOWS.iter().filter(|w| **w != label) {
        if let Some(window) = app.get_webview_window(other) {
            if window.is_visible().unwrap_or(false) {
                let _ = window.emit(&format!("{other}:reset"), ());
                let _ = window.hide();
            }
        }
    }
    let window = app
        .get_webview_window(label)
        .ok_or("Connection window not found")?;
    window
        .emit(&format!("{label}:{event}"), payload)
        .map_err(|e| format!("Failed to send open event: {e}"))?;
    window
        .show()
        .map_err(|e| format!("Failed to show connection window: {e}"))?;
    window
        .set_focus()
        .map_err(|e| format!("Failed to focus connection window: {e}"))?;
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
        WIN_REDIS => build_redis_menu(handle, &lang, &theme),
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

// ── Dev build marker ──

/// Whether this is a dev build. `debug_assertions` alone misses a release-profile build
/// made with `tauri.dev.conf.json`, so the `.debug` identifier counts too.
fn is_debug_build(app: &AppHandle) -> bool {
    cfg!(debug_assertions) || is_debug_identifier(&app.config().identifier)
}

/// `tauri.dev.conf.json` overrides the identifier with a `.debug` suffix.
fn is_debug_identifier(identifier: &str) -> bool {
    identifier.ends_with(".debug")
}

/// Marks a window title on dev builds so they can be told apart from the installed app.
fn decorate_title(base: &str, debug: bool) -> String {
    if debug {
        format!("[DEBUG] {base}")
    } else {
        base.to_owned()
    }
}

/// Sets a window title from its undecorated form. Every title change goes through
/// here so the dev marker is applied in one place.
fn apply_window_title(window: &tauri::WebviewWindow, base: &str) -> tauri::Result<()> {
    window.set_title(&decorate_title(base, is_debug_build(window.app_handle())))
}

/// Sets the calling window's title. The UI calls this instead of the JS `setTitle`.
#[tauri::command]
fn set_window_title(window: tauri::WebviewWindow, title: String) -> Result<(), String> {
    apply_window_title(&window, &title).map_err(|e| e.to_string())
}

// ── Menus ──

/// A menu item. Its id is `<window>:<item>` (`main:exit`), and the items Rust does not
/// handle itself reach that window's UI as the `<item>` part, in `menu:action`. An enum
/// so that the id the menu is built with and the one `on_menu_event` matches cannot
/// drift apart, and an item added without labels does not compile (#131).
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::IntoStaticStr, strum::EnumString)]
#[cfg_attr(test, derive(strum::EnumIter))]
#[strum(serialize_all = "kebab-case")]
enum MenuAction {
    // main
    NewProfile,
    NewGroup,
    Import,
    ImportTableplus,
    Export,
    SyncSettings,
    Exit,
    // Kept in the Store build (no self-updater), which only leaves it out of the menu.
    CheckUpdate,
    Manual,
    Github,
    OpenLogs,
    About,
    RememberDesktop,
    // query
    NewSqlTab,
    Close,
    SwitchDb,
    AiSettings,
    ToggleNotify,
    Run,
    RunAll,
    Cancel,
    Format,
    // settings
    TestConnection,
    Connect,
    Save,
    Delete,
    // The View menu of every window
    ThemeLight,
    ThemeDark,
    LangEn,
    LangJa,
}

impl MenuAction {
    /// The windows whose menu has this item. `menu_item` checks the menus against it,
    /// and `on_menu_event` ignores an id that pairs the item with another window.
    fn windows(self) -> &'static [&'static str] {
        use MenuAction::*;
        match self {
            NewProfile | NewGroup | Import | ImportTableplus | Export | SyncSettings | Exit
            | CheckUpdate | Manual | Github | OpenLogs | About | RememberDesktop => &[WIN_MAIN],
            NewSqlTab | SwitchDb | ToggleNotify | Run | RunAll | Cancel | Format => &[WIN_QUERY],
            AiSettings => &[WIN_QUERY, WIN_REDIS],
            Close => &CONNECTION_WINDOWS,
            TestConnection | Connect | Save | Delete => &[WIN_SETTINGS],
            ThemeLight | ThemeDark | LangEn | LangJa => {
                &[WIN_MAIN, WIN_QUERY, WIN_SETTINGS, WIN_REDIS]
            }
        }
    }

    /// What Rust does for this item, or `None` when the window's UI handles it (sent as
    /// `menu:action`). One exhaustive match, so a new item cannot compile until it picks
    /// a side, and an item for Rust cannot be left without its handler. The handler gets
    /// the label of the window whose menu it came from.
    fn rust_handler(self) -> Option<fn(&AppHandle, &str)> {
        use MenuAction::*;
        match self {
            Exit => Some(|app, _| quit(app)),
            Github => Some(|_, _| {
                let _ = open_in_browser("https://github.com/kan/musql");
            }),
            OpenLogs => Some(|app, _| {
                if let Err(e) = app_log::open_dir(app) {
                    log::warn!("failed to open the log directory: {e}");
                }
            }),
            // The popup menu is built afresh each time, from the setting (`build_main_menu`).
            RememberDesktop => Some(|app, _| {
                if let Err(e) = vdesk::set_enabled(app, !vdesk::is_enabled(app)) {
                    log::warn!("failed to save the virtual desktop setting: {e}");
                }
            }),
            // Not in the Store build's menu.
            #[cfg(not(feature = "self-updater"))]
            CheckUpdate => Some(|_, _| {}),
            #[cfg(feature = "self-updater")]
            CheckUpdate => Some(|app, _| {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    // A newer version announces itself (`update-available`). Both "up to
                    // date" and a failed check are "no update" to the user, as before;
                    // the failure is in the log.
                    if !matches!(check_update(handle.clone()).await, Ok(true)) {
                        let _ = handle.emit_to(
                            EventTarget::webview_window(WIN_MAIN),
                            "menu:action",
                            "no-update",
                        );
                    }
                });
            }),
            Close => Some(close_connection_window),
            NewProfile | NewGroup | Import | ImportTableplus | Export | SyncSettings | Manual
            | About | NewSqlTab | SwitchDb | AiSettings | ToggleNotify | Run | RunAll | Cancel
            | Format | TestConnection | Connect | Save | Delete | ThemeLight | ThemeDark
            | LangEn | LangJa => None,
        }
    }

    /// Whether the window's UI handles this item. A test checks it against the `case`s in
    /// each window's script.
    #[cfg(test)]
    fn handled_by_ui(self) -> bool {
        self.rust_handler().is_none()
    }
}

/// A submenu of the menu bar / popup menu.
#[derive(Clone, Copy)]
enum MenuTitle {
    File,
    Edit,
    View,
    Help,
    Query,
    Settings,
}

/// Picks the label for the UI language from an (English, Japanese) pair.
fn pick_label(lang: &str, (en, ja): (&'static str, &'static str)) -> &'static str {
    if lang == "ja" {
        ja
    } else {
        en
    }
}

fn menu_title(lang: &str, title: MenuTitle) -> &'static str {
    pick_label(
        lang,
        match title {
            MenuTitle::File => ("File", "ファイル"),
            MenuTitle::Edit => ("Edit", "編集"),
            MenuTitle::View => ("View", "表示"),
            MenuTitle::Help => ("Help", "ヘルプ"),
            MenuTitle::Query => ("Query", "クエリ"),
            MenuTitle::Settings => ("Settings", "設定"),
        },
    )
}

/// Each item names both languages in one arm, so neither label can be left out.
fn menu_label(lang: &str, item: MenuAction) -> &'static str {
    use MenuAction::*;
    pick_label(
        lang,
        match item {
            NewProfile => ("New Profile", "新規プロファイル"),
            NewGroup => ("New Group", "新規グループ"),
            Import => ("Import Profiles...", "インポート..."),
            ImportTableplus => ("Import from TablePlus...", "TablePlus から取り込む..."),
            Export => ("Export Profiles...", "エクスポート..."),
            SyncSettings => ("Sync Connections...", "接続の同期..."),
            Exit => ("Exit", "終了"),
            CheckUpdate => ("Check for Updates...", "アップデートを確認..."),
            Manual => ("Manual", "マニュアル"),
            Github => ("GitHub Repository", "GitHub リポジトリ"),
            OpenLogs => ("Open Log Folder", "ログフォルダを開く"),
            About => ("About muSQL", "muSQL について"),
            RememberDesktop => ("Remember Virtual Desktop", "仮想デスクトップを記憶"),
            NewSqlTab => ("New SQL Tab", "新規 SQL タブ"),
            Close => ("Close Window", "ウィンドウを閉じる"),
            SwitchDb => ("Switch Database", "データベース切替"),
            AiSettings => ("AI Settings", "AI 設定"),
            ToggleNotify => ("Notify on Query Completion", "クエリ完了を通知"),
            Run => ("Run", "実行"),
            RunAll => ("Run All", "すべて実行"),
            Cancel => ("Cancel", "キャンセル"),
            Format => ("Format", "整形"),
            TestConnection => ("Test Connection", "接続テスト"),
            Connect => ("Connect", "接続"),
            Save => ("Save", "保存"),
            Delete => ("Delete", "削除"),
            ThemeLight => ("Light", "ライト"),
            ThemeDark => ("Dark", "ダーク"),
            // Each language is listed under its own name.
            LangEn => ("English", "English"),
            LangJa => ("日本語", "日本語"),
        },
    )
}

fn menu_id(window: &str, item: MenuAction) -> String {
    let item: &'static str = item.into();
    format!("{window}:{item}")
}

/// The reverse of `menu_id`. `None` for ids that are not ours (the predefined items) and
/// for an item paired with a window whose menu does not have it.
fn parse_menu_id(id: &str) -> Option<(&str, MenuAction)> {
    let (window, item) = id.split_once(':')?;
    let action: MenuAction = item.parse().ok()?;
    action
        .windows()
        .contains(&window)
        .then_some((window, action))
}

fn menu_item(
    handle: &AppHandle<Wry>,
    lang: &str,
    window: &str,
    item: MenuAction,
    accelerator: Option<&str>,
) -> tauri::Result<MenuItem<Wry>> {
    debug_assert!(
        item.windows().contains(&window),
        "{item:?} in the {window} menu"
    );
    MenuItem::with_id(
        handle,
        menu_id(window, item),
        menu_label(lang, item),
        true,
        accelerator,
    )
}

fn check_menu_item(
    handle: &AppHandle<Wry>,
    lang: &str,
    window: &str,
    item: MenuAction,
    checked: bool,
) -> tauri::Result<CheckMenuItem<Wry>> {
    debug_assert!(
        item.windows().contains(&window),
        "{item:?} in the {window} menu"
    );
    CheckMenuItemBuilder::with_id(menu_id(window, item), menu_label(lang, item))
        .checked(checked)
        .build(handle)
}

fn build_edit_submenu(handle: &AppHandle<Wry>, lang: &str) -> tauri::Result<Submenu<Wry>> {
    Submenu::with_items(
        handle,
        menu_title(lang, MenuTitle::Edit),
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
    let check = |item, checked| check_menu_item(handle, lang, prefix, item, checked);
    Ok([
        Box::new(check(MenuAction::ThemeLight, is_light)?),
        Box::new(check(MenuAction::ThemeDark, !is_light)?),
        Box::new(PredefinedMenuItem::separator(handle)?),
        Box::new(check(MenuAction::LangEn, !is_ja)?),
        Box::new(check(MenuAction::LangJa, is_ja)?),
        Box::new(PredefinedMenuItem::separator(handle)?),
    ])
}

fn build_view_submenu(
    handle: &AppHandle<Wry>,
    lang: &str,
    theme: &str,
    prefix: &str,
    leading: &[&dyn tauri::menu::IsMenuItem<Wry>],
) -> tauri::Result<Submenu<Wry>> {
    let items = build_view_items(handle, lang, theme, prefix)?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> = leading
        .iter()
        .copied()
        .chain(items.iter().map(|b| b.as_ref()))
        .collect();
    Submenu::with_items(handle, menu_title(lang, MenuTitle::View), true, &refs)
}

fn build_main_menu(handle: &AppHandle<Wry>, lang: &str, theme: &str) -> tauri::Result<Menu<Wry>> {
    let item = |item, accelerator| menu_item(handle, lang, WIN_MAIN, item, accelerator);
    let file_menu = Submenu::with_items(
        handle,
        menu_title(lang, MenuTitle::File),
        true,
        &[
            &item(MenuAction::NewProfile, Some("CmdOrCtrl+N"))?,
            &item(MenuAction::NewGroup, Some("CmdOrCtrl+Shift+N"))?,
            &PredefinedMenuItem::separator(handle)?,
            &item(MenuAction::Import, None)?,
            &item(MenuAction::ImportTableplus, None)?,
            &item(MenuAction::Export, None)?,
            &item(MenuAction::SyncSettings, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &item(MenuAction::Exit, None)?,
        ],
    )?;
    let edit_menu = build_edit_submenu(handle, lang)?;
    // Read each time the menu is built: the popup is built afresh on every opening.
    let remember_desktop = check_menu_item(
        handle,
        lang,
        WIN_MAIN,
        MenuAction::RememberDesktop,
        vdesk::is_enabled(handle),
    )?;
    let remember_sep = PredefinedMenuItem::separator(handle)?;
    let view_menu = build_view_submenu(
        handle,
        lang,
        theme,
        WIN_MAIN,
        &[&remember_desktop, &remember_sep],
    )?;
    let manual_item = item(MenuAction::Manual, None)?;
    let github_item = item(MenuAction::Github, None)?;
    let open_logs_item = item(MenuAction::OpenLogs, None)?;
    let about_sep = PredefinedMenuItem::separator(handle)?;
    let about_item = item(MenuAction::About, None)?;
    #[cfg(feature = "self-updater")]
    let help_menu = {
        let check_update_item = item(MenuAction::CheckUpdate, None)?;
        let sep = PredefinedMenuItem::separator(handle)?;
        Submenu::with_items(
            handle,
            menu_title(lang, MenuTitle::Help),
            true,
            &[
                &check_update_item,
                &sep,
                &manual_item,
                &github_item,
                &open_logs_item,
                &about_sep,
                &about_item,
            ],
        )?
    };
    #[cfg(not(feature = "self-updater"))]
    let help_menu = Submenu::with_items(
        handle,
        menu_title(lang, MenuTitle::Help),
        true,
        &[
            &manual_item,
            &github_item,
            &open_logs_item,
            &about_sep,
            &about_item,
        ],
    )?;
    Menu::with_items(handle, &[&file_menu, &edit_menu, &view_menu, &help_menu])
}

fn build_query_menu(
    handle: &AppHandle<Wry>,
    lang: &str,
    theme: &str,
    notify: bool,
) -> tauri::Result<Menu<Wry>> {
    let item = |item, accelerator| menu_item(handle, lang, WIN_QUERY, item, accelerator);
    let file_menu = Submenu::with_items(
        handle,
        menu_title(lang, MenuTitle::File),
        true,
        &[
            &item(MenuAction::NewSqlTab, Some("CmdOrCtrl+T"))?,
            &PredefinedMenuItem::separator(handle)?,
            &item(MenuAction::Close, Some("CmdOrCtrl+W"))?,
        ],
    )?;
    let edit_menu = build_edit_submenu(handle, lang)?;
    // Query view menu: Switch DB + shared theme/lang items
    let switch_db = item(MenuAction::SwitchDb, None)?;
    let ai_settings = item(MenuAction::AiSettings, None)?;
    let ai_sep = PredefinedMenuItem::separator(handle)?;
    let notify_item = check_menu_item(handle, lang, WIN_QUERY, MenuAction::ToggleNotify, notify)?;
    let notify_sep = PredefinedMenuItem::separator(handle)?;
    let view_menu = build_view_submenu(
        handle,
        lang,
        theme,
        WIN_QUERY,
        &[&switch_db, &ai_settings, &ai_sep, &notify_item, &notify_sep],
    )?;
    let query_menu = Submenu::with_items(
        handle,
        menu_title(lang, MenuTitle::Query),
        true,
        &[
            &item(MenuAction::Run, Some("CmdOrCtrl+Enter"))?,
            &item(MenuAction::RunAll, Some("CmdOrCtrl+Shift+Enter"))?,
            &item(MenuAction::Cancel, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &item(MenuAction::Format, Some("CmdOrCtrl+Shift+F"))?,
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
    let view_menu = build_view_submenu(handle, lang, theme, WIN_SETTINGS, &[])?;
    let item = |item| menu_item(handle, lang, WIN_SETTINGS, item, None);
    let settings_menu = Submenu::with_items(
        handle,
        menu_title(lang, MenuTitle::Settings),
        true,
        &[
            &item(MenuAction::TestConnection)?,
            &item(MenuAction::Connect)?,
            &PredefinedMenuItem::separator(handle)?,
            &item(MenuAction::Save)?,
            &item(MenuAction::Delete)?,
        ],
    )?;
    Menu::with_items(handle, &[&edit_menu, &view_menu, &settings_menu])
}

/// The windows that show an open connection: a profile opens in one of them, chosen by
/// `Engine::window`.
const CONNECTION_WINDOWS: [&str; 2] = [WIN_QUERY, WIN_REDIS];

/// Closes a connection window: its page drops the connection (`<label>:reset`), the
/// window hides, and the connections list comes back. From the window's close button
/// and from its menu alike.
fn close_connection_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.emit(&format!("{label}:reset"), ());
        let _ = w.hide();
    }
    if let Some(w) = app.get_webview_window(WIN_MAIN) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn build_redis_menu(handle: &AppHandle<Wry>, lang: &str, theme: &str) -> tauri::Result<Menu<Wry>> {
    let file_menu = Submenu::with_items(
        handle,
        menu_title(lang, MenuTitle::File),
        true,
        &[&menu_item(
            handle,
            lang,
            WIN_REDIS,
            MenuAction::Close,
            Some("CmdOrCtrl+W"),
        )?],
    )?;
    let edit_menu = build_edit_submenu(handle, lang)?;
    // View: AI settings + shared theme/lang items
    let ai_settings = menu_item(handle, lang, WIN_REDIS, MenuAction::AiSettings, None)?;
    let ai_sep = PredefinedMenuItem::separator(handle)?;
    let view_menu = build_view_submenu(handle, lang, theme, WIN_REDIS, &[&ai_settings, &ai_sep])?;
    Menu::with_items(handle, &[&file_menu, &edit_menu, &view_menu])
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
    if let Some(w) = handle.get_webview_window(WIN_REDIS) {
        let _ = w.set_menu(build_redis_menu(handle, "en", "light")?);
        let _ = w.hide_menu();
    }
    Ok(())
}

/// Asks the update server for a newer version. Failures are logged here, so every
/// caller records them the same way.
#[cfg(feature = "self-updater")]
async fn fetch_update(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    use tauri_plugin_updater::UpdaterExt;
    let result = match app.updater() {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    // Throttled: the periodic check repeats the same failure every hour while offline.
    result.inspect_err(|e| {
        app_log::warn_throttled(app_log::LONG, &format!("update check failed: {e}"))
    })
}

/// How often a running app looks for a newer version (#114). A database client tends to
/// stay open for days, so checking only at startup misses whatever ships meanwhile.
#[cfg(feature = "self-updater")]
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Checks for an update and tells the UI the outcome, whichever it is:
/// `update-available` with the version, or `update-none`. The UI replaces what it shows
/// on every check, so it follows the latest answer instead of the first one it got.
///
/// A failed check emits nothing: an update found earlier stays on screen.
#[cfg(feature = "self-updater")]
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<bool, String> {
    match fetch_update(&app).await? {
        Some(update) => {
            let _ = app.emit(
                "update-available",
                serde_json::json!({ "version": update.version }),
            );
            Ok(true)
        }
        None => {
            let _ = app.emit("update-none", ());
            Ok(false)
        }
    }
}

#[cfg(feature = "self-updater")]
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    let update = fetch_update(&app).await?;
    if let Some(update) = update {
        update
            .download_and_install(|_, _| {}, || {})
            .await
            .map_err(|e| e.to_string())
            .inspect_err(|e| log::error!("update install failed: {e}"))?;
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
    /// From the container's detection (`DockerContainer::engine`).
    engine: Engine,
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
    Err("Docker is not reachable".to_owned())
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
    docker::discovery::discover_containers(&docker).await
}

#[cfg(feature = "docker")]
#[tauri::command]
async fn docker_create_tunnel(
    container_id: String,
    port: u16,
) -> Result<docker::tunnel::TunnelInfo, String> {
    let docker = connect_docker().await?;
    docker::tunnel::ensure_socat_image(&docker)
        .await
        .inspect_err(|e| log::warn!("docker tunnel image unavailable: {e}"))?;
    docker::tunnel::create_tunnel(&docker, &container_id, port)
        .await
        .inspect_err(|e| log::warn!("docker tunnel to {container_id}:{port} failed: {e}"))
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

/// Opens a Docker container in the window for its engine: `query:docker-open` or
/// `redis:docker-open`.
#[cfg(feature = "docker")]
#[tauri::command]
fn open_docker_window(app: AppHandle, info: DockerConnectionInfo) -> Result<(), String> {
    // An engine this version does not know opens no window.
    let label = info.engine.window()?;
    show_connection_window(&app, label, "docker-open", &info)
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
fn open_docker_window(_app: AppHandle, _info: DockerConnectionInfo) -> Result<(), String> {
    Ok(())
}

/// Ends the app: from the menu's Exit, and when the main window is closed.
fn quit(app: &AppHandle) {
    // std::process::exit bypasses the plugin's save-on-exit hook,
    // so persist window state explicitly first (#42).
    {
        use tauri_plugin_window_state::{AppHandleExt, StateFlags};
        let _ = app.save_window_state(StateFlags::all());
    }
    vdesk::on_quit(app, &[&[WIN_MAIN][..], &CONNECTION_WINDOWS].concat());
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
            // First, so that failures in the rest of setup are written too.
            app_log::init(app.handle());
            setup_menus(app.handle())?;
            // Back to the virtual desktop main was last on (#126), when that is turned on
            // (#144). The mutex is taken either way (see `vdesk::claim`).
            vdesk::claim(app.handle());
            if let Some(main_win) = app.get_webview_window(WIN_MAIN) {
                vdesk::restore(app.handle(), &main_win);
            }
            // The titles in tauri.conf.json are the release ones; mark them on dev builds.
            // Runs once, while every title is still the undecorated one from the config.
            if is_debug_build(app.handle()) {
                for win in app.webview_windows().values() {
                    if let Ok(title) = win.title() {
                        let _ = apply_window_title(win, &title);
                    }
                }
            }
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
                    // Then for as long as the app runs. Each check tells the UI its
                    // outcome; a failure is logged and leaves the UI as it was.
                    loop {
                        let _ = check_update(handle.clone()).await;
                        tokio::time::sleep(UPDATE_CHECK_INTERVAL).await;
                    }
                });
            }
            Ok(())
        })
        .on_menu_event(|app, event| {
            let Some((window_label, action)) = parse_menu_id(event.id().0.as_str()) else {
                return;
            };
            match action.rust_handler() {
                Some(handle) => handle(app, window_label),
                // The UI of the window the menu belongs to handles it.
                None => {
                    let _ = app.emit_to(
                        EventTarget::webview_window(window_label),
                        "menu:action",
                        <&'static str>::from(action),
                    );
                }
            }
        })
        .manage(Arc::new(Mutex::new(None::<ConnectionCache>)))
        .manage(redis_client::RedisState::default())
        .manage(ActiveWindow(Mutex::new(WIN_MAIN.to_owned())))
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(true) => {
                if let Some(state) = window.try_state::<ActiveWindow>() {
                    if let Ok(mut label) = state.0.lock() {
                        *label = window.label().to_owned();
                    }
                }
            }
            // Closing main ends the app. Without this the process lives on: the settings
            // and query windows are only ever hidden, and Tauri exits by itself only
            // once every window is gone.
            tauri::WindowEvent::CloseRequested { .. } if window.label() == WIN_MAIN => {
                quit(window.app_handle());
            }
            tauri::WindowEvent::CloseRequested { api, .. } if window.label() != WIN_MAIN => {
                api.prevent_close();
                if CONNECTION_WINDOWS.contains(&window.label()) {
                    close_connection_window(window.app_handle(), window.label());
                } else {
                    let _ = window.hide();
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
            open_connection_window,
            hide_window,
            disconnect_pool,
            close_query_tab,
            redis_connect,
            redis_disconnect,
            redis_scan,
            redis_key,
            redis_command,
            redis_select,
            redis_databases,
            redis_command_docs,
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
            import_tableplus,
            get_sync_path,
            set_sync_path,
            sync_import,
            sync_export,
            pick_sync_path,
            sync_folder_candidates,
            show_popup_menu,
            check_update,
            install_update,
            ai_complete,
            ai_assist,
            redis_ai_assist,
            save_ai_api_key,
            has_ai_api_key,
            clear_schema_cache,
            docker_available,
            docker_list_containers,
            docker_create_tunnel,
            docker_stop_tunnel,
            docker_cleanup_tunnels,
            open_docker_window,
            get_docker_password,
            save_docker_password,
            op_available,
            op_read_secret,
            op_list_items,
            op_list_fields,
            open_external,
            set_window_title,
            app_log::log_frontend,
            toast::toast_notify
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
            engine: Engine::Mysql,
            mysql: MySqlConfig {
                host: host.to_owned(),
                port,
                database: None,
                username: user.to_owned(),
                password: String::new(),
                ssl_mode: SslMode::Disabled,
                tls_ca_cert_path: None,
                save_password: true,
                op_ref: None,
                file_path: None,
                tls_enabled: false,
                tls_skip_verify: false,
            },
            ssh,
        }
    }

    // #133: through SSH, only VERIFY_IDENTITY changes anything. Redis gets its TLS from the
    // tunnel; MySQL is refused, as its host name cannot be checked there.
    #[test]
    fn verify_identity_through_ssh_is_made_by_the_tunnel_or_refused() {
        let mut req = make_request("db.example.com", 3306, "root", None);
        req.mysql.ssl_mode = SslMode::VerifyCa;
        assert!(tunnel_tls(&req).unwrap().is_none());

        req.mysql.ssl_mode = SslMode::VerifyIdentity;
        assert!(tunnel_tls(&req).is_err_and(|e| e.contains("VERIFY_CA")));

        // A server on the jump host itself is checked right by the client.
        let mut local = req.clone();
        local.mysql.host = "127.0.0.1".to_owned();
        assert!(tunnel_tls(&local).unwrap().is_none());

        req.engine = Engine::Redis;
        let tls = tunnel_tls(&req).unwrap().expect("the tunnel makes the TLS");
        assert_eq!(tls.server_name.to_str(), "db.example.com");
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
            host: "bastion.example.com".to_owned(),
            port: 22,
            username: "deploy".to_owned(),
            private_key_path: Some("/keys/id_rsa".to_owned()),
            config_host: None,
            passphrase: String::new(),
            auth_method: "key".to_owned(),
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

    // ── Menus ──────────────────────────────────────────────────────

    /// The `case "..."` labels of a script's `menu:action` listener: from the listener
    /// to the end of its `switch` (the first line that closes a block at its indent).
    fn menu_action_cases(script: &str) -> std::collections::BTreeSet<String> {
        let text = read_ui(script);
        let start = text
            .find("listen(\"menu:action\"")
            .unwrap_or_else(|| panic!("{script} has no menu:action listener"));
        let listener = &text[start..];
        // The indent of the line the listener starts on.
        let line = &text[text[..start].rfind('\n').map_or(0, |i| i + 1)..start];
        let indent = line.len() - line.trim_start_matches(' ').len();
        let end = listener
            .find(&format!("\n{}}});", " ".repeat(indent)))
            .unwrap_or_else(|| panic!("cannot find the end of {script}'s listener"));
        listener[..end]
            .split("case \"")
            .skip(1)
            .filter_map(|rest| rest.split_once('"').map(|(label, _)| label.to_owned()))
            .collect()
    }

    // Each window's script must handle exactly the items its menu hands to the UI. A
    // menu item with no `case` does nothing when chosen, and a `case` no item sends is
    // dead code, typically left by a rename on one side only.
    #[test]
    fn ui_scripts_handle_the_menu_items_they_are_sent() {
        use strum::IntoEnumIterator;
        for (window, script, extra) in [
            // Sent by `on_menu_event` after a manual update check finds nothing.
            (WIN_MAIN, "app.js", &["no-update"][..]),
            (WIN_QUERY, "query.js", &[][..]),
            (WIN_SETTINGS, "settings.js", &[][..]),
            (WIN_REDIS, "redis.js", &[][..]),
        ] {
            let mut expected: std::collections::BTreeSet<String> = MenuAction::iter()
                .filter(|a| a.handled_by_ui() && a.windows().contains(&window))
                .map(|a| <&'static str>::from(a).to_owned())
                .collect();
            expected.extend(extra.iter().map(|s| (*s).to_owned()));
            assert_eq!(menu_action_cases(script), expected, "{script}");
        }
    }

    #[test]
    fn menu_ids_round_trip() {
        use strum::IntoEnumIterator;
        for action in MenuAction::iter() {
            for window in action.windows() {
                let id = menu_id(window, action);
                assert_eq!(parse_menu_id(&id), Some((*window, action)), "{id}");
            }
        }
        // The predefined items (undo, copy, ...) carry ids that are not ours.
        assert_eq!(parse_menu_id("undo"), None);
        assert_eq!(parse_menu_id("main:no-such-item"), None);
        // An item paired with a window whose menu does not have it.
        assert_eq!(parse_menu_id("settings:exit"), None);
        assert_eq!(parse_menu_id("main:close"), None);
    }

    // ── Engine ─────────────────────────────────────────────────────

    // Profiles written before #132 have no `engine`; they are MySQL.
    #[test]
    fn engine_absent_means_mysql() {
        let request: ConnectionRequest = serde_json::from_str(
            r#"{"mysql":{"host":"h","port":3306,"database":null,"username":"u"},"ssh":null}"#,
        )
        .unwrap();
        assert_eq!(request.engine, Engine::Mysql);
    }

    #[test]
    fn engine_reads_and_writes_the_stored_names() {
        for (name, engine) in [("mysql", Engine::Mysql), ("mariadb", Engine::Mariadb)] {
            let json = format!("\"{name}\"");
            assert_eq!(serde_json::from_str::<Engine>(&json).unwrap(), engine);
            assert_eq!(serde_json::to_string(&engine).unwrap(), json);
            assert!(engine.ensure_mysql_protocol().is_ok());
            assert_eq!(engine.window(), Ok(WIN_QUERY));
        }
    }

    // SQLite opens in the query window, but never through the MySQL client (#97).
    #[test]
    fn sqlite_engine_opens_the_query_window_without_mysql() {
        let engine: Engine = serde_json::from_str("\"sqlite\"").unwrap();
        assert_eq!(engine, Engine::Sqlite);
        assert_eq!(serde_json::to_string(&engine).unwrap(), "\"sqlite\"");
        assert_eq!(engine.window(), Ok(WIN_QUERY));
        assert!(engine.ensure_mysql_protocol().is_err());
    }

    // PostgreSQL opens in the query window through its own client (#96).
    #[test]
    fn postgresql_engine_opens_the_query_window_without_mysql() {
        let engine: Engine = serde_json::from_str("\"postgresql\"").unwrap();
        assert_eq!(engine, Engine::Postgresql);
        assert_eq!(serde_json::to_string(&engine).unwrap(), "\"postgresql\"");
        assert_eq!(engine.window(), Ok(WIN_QUERY));
        let refused = engine.ensure_mysql_protocol().unwrap_err();
        assert!(refused.contains("PostgreSQL"), "{refused}");
    }

    // A PostgreSQL connection is to one database, so another database is another
    // connection; MySQL switches databases on the same one.
    #[test]
    fn fingerprint_tells_postgresql_databases_apart() {
        let request = |engine: &str, database: &str| -> ConnectionRequest {
            serde_json::from_value(json!({
                "engine": engine,
                "mysql": {"host": "h", "port": 1, "database": database, "username": "u"},
                "ssh": null,
            }))
            .unwrap()
        };
        assert_ne!(
            connection_fingerprint(&request("postgresql", "a")),
            connection_fingerprint(&request("postgresql", "b"))
        );
        assert_eq!(
            connection_fingerprint(&request("mysql", "a")),
            connection_fingerprint(&request("mysql", "b"))
        );
    }

    // An engine from a later version must keep its spelling through a save (the sync
    // mirror would otherwise turn it into MySQL on every machine), and must never be
    // connected to as MySQL.
    #[test]
    fn unknown_engine_round_trips_and_opens_nothing() {
        let engine: Engine = serde_json::from_str("\"oracle\"").unwrap();
        assert_eq!(engine, Engine::Unknown("oracle".to_owned()));
        assert_eq!(serde_json::to_string(&engine).unwrap(), "\"oracle\"");
        assert!(engine.window().is_err());
        assert!(engine.ensure_mysql_protocol().is_err());
    }

    /// Every engine this version knows, with its stored name.
    fn known_engines() -> Vec<(String, Engine)> {
        let engines = [
            Engine::Mysql,
            Engine::Mariadb,
            Engine::Redis,
            Engine::Sqlite,
            Engine::Postgresql,
        ];
        for engine in &engines {
            // No wildcard: a new variant stops this compiling until it is listed above.
            match engine {
                Engine::Mysql
                | Engine::Mariadb
                | Engine::Redis
                | Engine::Sqlite
                | Engine::Postgresql
                | Engine::Unknown(_) => {}
            }
        }
        engines
            .into_iter()
            .map(|e| (String::from(e.clone()), e))
            .collect()
    }

    fn read_ui(file: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../ui")
            .join(file);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }

    /// The text between `start` (exclusive) and the next `end`.
    fn between<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
        let from = text
            .find(start)
            .unwrap_or_else(|| panic!("{start:?} not found"))
            + start.len();
        let len = text[from..]
            .find(end)
            .unwrap_or_else(|| panic!("{end:?} not found"));
        &text[from..from + len]
    }

    // The UI names the engines in two places of its own: the list's labels (app.js) and
    // the settings window's choices. Both must match `Engine` (#132), or the UI would
    // offer an engine Rust treats as unknown, or name one differently.
    #[test]
    fn ui_engine_names_match_the_engines() {
        let expected: Vec<(String, String)> = known_engines()
            .into_iter()
            .map(|(id, e)| (id, e.display_name().to_owned()))
            .collect();

        // `const ENGINE_NAMES = { mysql: "MySQL", ... };`
        let names: Vec<(String, String)> =
            between(&read_ui("app.js"), "const ENGINE_NAMES = {", "};")
                .split(',')
                .filter_map(|pair| pair.split_once(':'))
                .map(|(id, name)| {
                    (
                        id.trim().to_owned(),
                        name.trim().trim_matches('"').to_owned(),
                    )
                })
                .collect();
        assert_eq!(names, expected, "ENGINE_NAMES in app.js");

        // `<option value="mysql" selected>MySQL</option>`
        let options: Vec<(String, String)> = between(
            &read_ui("settings.html"),
            "<select id=\"db-engine\">",
            "</select>",
        )
        .split("<option value=\"")
        .skip(1)
        .map(|option| {
            let (id, rest) = option.split_once('"').expect("value is quoted");
            let name = between(rest, ">", "</option>");
            (id.to_owned(), name.to_owned())
        })
        .collect();
        assert_eq!(options, expected, "the DB Engine choices in settings.html");

        // `const ENGINE_FIELDS = { mysql: ..., ... };` in settings.js: the per-engine
        // defaults. An engine missing here would quietly get MySQL's port and labels.
        let settings = read_ui("settings.js");
        // The keys at the top level of the object; an entry may span lines.
        let mut fields: Vec<String> = Vec::new();
        let mut depth = 0;
        for line in between(&settings, "const ENGINE_FIELDS = {", "\n};").lines() {
            if depth == 0 {
                if let Some((id, _)) = line.trim().split_once(':') {
                    fields.push(id.trim().to_owned());
                }
            }
            depth += line.matches('{').count();
            depth -= line.matches('}').count();
        }
        let ids: Vec<String> = expected.into_iter().map(|(id, _)| id).collect();
        assert_eq!(fields, ids, "ENGINE_FIELDS in settings.js");
    }

    // ── SslMode ────────────────────────────────────────────────────

    // Saved profiles and the UI's choices use these spellings; they must keep loading
    // and be written back unchanged.
    #[test]
    fn ssl_mode_reads_and_writes_the_stored_names() {
        for (name, mode) in [
            ("DISABLED", SslMode::Disabled),
            ("PREFERRED", SslMode::Preferred),
            ("REQUIRED", SslMode::Required),
            ("VERIFY_CA", SslMode::VerifyCa),
            ("VERIFY_IDENTITY", SslMode::VerifyIdentity),
        ] {
            let json = format!("\"{name}\"");
            assert_eq!(serde_json::from_str::<SslMode>(&json).unwrap(), mode);
            assert_eq!(serde_json::to_string(&mode).unwrap(), json);
        }
    }

    /// A MySQL server without TLS that greets each connection (a handshake without
    /// CLIENT_SSL) and hangs up on whatever the client says next.
    fn mysql_server_without_tls() -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                // LONG_PASSWORD | PROTOCOL_41 | SECURE_CONNECTION | PLUGIN_AUTH; no SSL.
                let caps: u32 = 0x0001 | 0x0200 | 0x8000 | 0x0008_0000;
                let mut payload = vec![10];
                payload.extend_from_slice(b"8.0.0\0");
                payload.extend_from_slice(&1u32.to_le_bytes());
                payload.extend_from_slice(b"abcdefgh\0");
                payload.extend_from_slice(&(caps as u16).to_le_bytes());
                payload.push(33);
                payload.extend_from_slice(&2u16.to_le_bytes());
                payload.extend_from_slice(&((caps >> 16) as u16).to_le_bytes());
                payload.push(21);
                payload.extend_from_slice(&[0; 10]);
                payload.extend_from_slice(b"ijklmnopqrst\0");
                payload.extend_from_slice(b"mysql_native_password\0");
                let len = (payload.len() as u32).to_le_bytes();
                let mut packet = vec![len[0], len[1], len[2], 0];
                packet.extend_from_slice(&payload);
                let _ = stream.write_all(&packet);
                let _ = stream.read(&mut [0u8; 1024]);
            }
        });
        port
    }

    #[test]
    fn preferred_mysql_retries_without_tls_only_when_the_server_has_none() {
        let port = mysql_server_without_tls();
        let mut req = make_request("127.0.0.1", port, "root", None);
        let not_supported = "server does not have this capability";
        req.mysql.ssl_mode = SslMode::Required;
        let required = connect_mysql_pool(&req.mysql, "127.0.0.1", port)
            .err()
            .unwrap();
        assert!(required.contains(not_supported), "{required}");
        // PREFERRED gets past the TLS check and fails later (this server hangs up).
        req.mysql.ssl_mode = SslMode::Preferred;
        let preferred = connect_mysql_pool(&req.mysql, "127.0.0.1", port)
            .err()
            .unwrap();
        assert!(!preferred.contains(not_supported), "{preferred}");
    }

    #[test]
    fn ssl_mode_absent_means_disabled() {
        let mysql: MySqlConfig =
            serde_json::from_str(r#"{"host":"h","port":3306,"database":null,"username":"u"}"#)
                .unwrap();
        assert_eq!(mysql.ssl_mode, SslMode::Disabled);
    }

    // An unknown value must not fail the load (it would take every profile with it),
    // must not connect without TLS, and must survive being saved again (the sync mirror
    // would otherwise carry the loss to the machine that wrote it).
    #[test]
    fn unknown_ssl_mode_loads_round_trips_and_refuses_to_connect() {
        for name in ["VERIFY_FULL", "verify_ca", ""] {
            let json = format!("\"{name}\"");
            let mode: SslMode = serde_json::from_str(&json).unwrap();
            assert_eq!(mode, SslMode::Unknown(name.to_owned()));
            assert_eq!(serde_json::to_string(&mode).unwrap(), json);
        }
        let mut req = make_request("127.0.0.1", 3306, "root", None);
        assert!(build_opts(&req.mysql, "127.0.0.1", 3306).is_ok());
        req.mysql.ssl_mode = SslMode::Unknown("VERIFY_FULL".to_owned());
        assert!(build_opts(&req.mysql, "127.0.0.1", 3306).is_err());
    }

    // ── generate_profile_id ────────────────────────────────────────

    #[test]
    fn keep_local_settings_carries_what_the_other_app_does_not_know() {
        let mut existing = make_request("old", 3306, "u", Some(make_ssh(None)));
        existing.mysql.op_ref = Some("op://v/i/password".to_owned());
        if let Some(ssh) = existing.ssh.as_mut() {
            ssh.save_ssh_passphrase = false;
            ssh.op_passphrase_ref = Some("op://v/i/passphrase".to_owned());
        }
        let mut imported = make_request("new", 3307, "u2", Some(make_ssh(None)));
        imported.mysql.save_password = false;
        keep_local_settings(&existing, &mut imported);
        // The connection is the imported one; how its secrets are kept is not.
        assert_eq!(
            (imported.mysql.host.as_str(), imported.mysql.port),
            ("new", 3307)
        );
        assert!(imported.mysql.save_password);
        assert_eq!(imported.mysql.op_ref.as_deref(), Some("op://v/i/password"));
        let ssh = imported.ssh.unwrap();
        assert!(!ssh.save_ssh_passphrase);
        assert_eq!(
            ssh.op_passphrase_ref.as_deref(),
            Some("op://v/i/passphrase")
        );
    }

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

    // ── decorate_title ───────────────────────────────────────────────

    #[test]
    fn decorate_title_marks_only_debug_builds() {
        assert_eq!(decorate_title("muSQL", true), "[DEBUG] muSQL");
        assert_eq!(decorate_title("muSQL", false), "muSQL");
    }

    #[test]
    fn is_debug_identifier_matches_the_dev_config_suffix() {
        // Keep in step with the identifier in tauri.dev.conf.json.
        assert!(is_debug_identifier("jp.co.communitylinks.musql.debug"));
        assert!(!is_debug_identifier("jp.co.communitylinks.musql"));
    }

    // ── build_ai_prompt ──────────────────────────────────────────────

    #[test]
    fn build_ai_prompt_basic() {
        let schema = SchemaInfo {
            database: "testdb".to_owned(),
            tables: vec![SchemaTable {
                name: "users".to_owned(),
                columns: vec![
                    SchemaColumn {
                        name: "id".to_owned(),
                        data_type: "int".to_owned(),
                        column_key: "PRI".to_owned(),
                        is_nullable: "NO".to_owned(),
                    },
                    SchemaColumn {
                        name: "name".to_owned(),
                        data_type: "varchar".to_owned(),
                        column_key: "".to_owned(),
                        is_nullable: "YES".to_owned(),
                    },
                ],
            }],
        };
        let prompt = build_ai_prompt(&Engine::Mysql, &schema, "SELECT ", "");
        assert!(prompt.contains("You are a MySQL query assistant"));
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
            database: "bigdb".to_owned(),
            tables: (0..200)
                .map(|i| SchemaTable {
                    name: format!("table_{i}"),
                    columns: (0..20)
                        .map(|j| SchemaColumn {
                            name: format!("column_{j}_with_a_longer_name"),
                            data_type: "varchar".to_owned(),
                            column_key: "".to_owned(),
                            is_nullable: "YES".to_owned(),
                        })
                        .collect(),
                })
                .collect(),
        };
        let prompt = build_ai_prompt(&Engine::Mysql, &schema, "SELECT ", "");
        assert!(prompt.contains("(truncated)"));
    }

    #[test]
    fn code_fences_are_stripped_from_ai_replies() {
        assert_eq!(strip_code_fence("SELECT 1"), "SELECT 1");
        assert_eq!(strip_code_fence("```sql\nSELECT 1\n```"), "SELECT 1");
        assert_eq!(
            strip_code_fence("```redis\nGET a\nGET b\n```\n"),
            "GET a\nGET b"
        );
        assert_eq!(strip_code_fence("```\nGET a```"), "GET a");
        assert_eq!(strip_code_fence("```sql SELECT 1```"), "SELECT 1");
        assert_eq!(strip_code_fence("```GET a```"), "GET a");
    }

    #[test]
    fn redis_ai_prompt_carries_keys_and_the_request() {
        let keys: String = (0..2000).map(|i| format!("user:{i} (hash)\n")).collect();
        let prompt = build_redis_ai_prompt("7.4.1", 3, keys, "count the users", "User: hi");
        assert!(prompt.contains("Redis version: 7.4.1"));
        assert!(prompt.contains("Current database: 3"));
        assert!(prompt.contains("user:0 (hash)\n"));
        assert!(prompt.contains("(truncated)"));
        assert!(prompt.contains("Previous conversation:\nUser: hi"));
        assert!(prompt.ends_with("User request: count the users"));
    }

    #[test]
    fn build_ai_prompt_truncation_multibyte() {
        // 3-byte chars put the 8000-byte cut inside a character.
        let schema = SchemaInfo {
            database: "bigdb".to_owned(),
            tables: vec![SchemaTable {
                name: "顧客".repeat(2000),
                columns: vec![],
            }],
        };
        let prompt = build_ai_prompt(&Engine::Mysql, &schema, "SELECT ", "");
        assert!(prompt.contains("(truncated)"));
        let prompt = build_ai_assist_prompt(&Engine::Mariadb, &schema, "list", "", "");
        assert!(prompt.contains("You are a MariaDB query assistant"));
        assert!(prompt.contains("(truncated)"));

        // A single over-long line has no earlier newline, so the cut lands on the
        // nearest char boundary at or below the limit.
        let text = schema_text_for_prompt(&schema);
        let kept = text.strip_suffix("\n-- (truncated)\n").unwrap().len();
        assert!((AI_CONTEXT_TEXT_MAX - 2..=AI_CONTEXT_TEXT_MAX).contains(&kept));
    }

    #[test]
    fn schema_text_for_prompt_cuts_on_line_boundary() {
        let schema = SchemaInfo {
            database: "bigdb".to_owned(),
            tables: (0..1000)
                .map(|i| SchemaTable {
                    name: format!("table_{i}"),
                    columns: vec![],
                })
                .collect(),
        };
        let text = schema_text_for_prompt(&schema);
        let kept = text.strip_suffix("\n-- (truncated)\n").unwrap();
        assert!(kept.len() <= AI_CONTEXT_TEXT_MAX);
        assert!(kept.ends_with('\n'));
        assert!(kept.lines().all(|l| l.starts_with("-- table_")));
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
        ssh.auth_method = auth_method.to_owned();
        ssh.op_passphrase_ref = Some(NO_SUCH_REF.to_owned());
        ssh.op_password_ref = Some(NO_SUCH_REF.to_owned());
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
        req.mysql.op_ref = Some(NO_SUCH_REF.to_owned());
        req.mysql.save_password = false;
        resolve_password(&mut req, Some(UNUSED_ID)).unwrap();
        assert!(req.mysql.password.is_empty());
    }

    #[test]
    fn resolve_password_prefers_a_value_already_on_the_request() {
        // What the per-connection prompt collected wins over both keyring and 1Password.
        let mut req = make_request("127.0.0.1", 3306, "root", None);
        req.mysql.op_ref = Some(NO_SUCH_REF.to_owned());
        req.mysql.password = "typed-at-the-prompt".to_owned();
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
            host: "bastion.example.com".to_owned(),
            port: 22,
            username: "deploy".to_owned(),
            private_key_path: private_key_path.map(str::to_owned),
            config_host: Some("myserver".to_owned()),
            passphrase: String::new(),
            auth_method: "key".to_owned(),
            ssh_password: String::new(),
            save_ssh_password: true,
            save_ssh_passphrase: true,
            op_passphrase_ref: None,
            op_password_ref: None,
        }
    }

    fn make_profile(id: &str, ssh: Option<SshConfig>) -> ConnectionProfile {
        ConnectionProfile {
            id: id.to_owned(),
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
        profile.request.mysql.tls_ca_cert_path = Some("C:\\Users\\alice\\certs\\ca.pem".to_owned());
        profile.request.mysql.file_path = Some("C:\\Users\\alice\\data\\app.db".to_owned());
        let store = ConnectionProfileStore {
            version: 1,
            app_version: None,
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
        profile.request.mysql.password = "mysql-secret".to_owned();
        let ssh = profile.request.ssh.as_mut().unwrap();
        ssh.passphrase = "passphrase-secret".to_owned();
        ssh.ssh_password = "ssh-secret".to_owned();
        let store = ConnectionProfileStore {
            version: 1,
            app_version: None,
            groups: vec![],
            items: vec![profile],
        };

        let json = sanitized_store_json(&store).unwrap();
        assert!(!json.contains("secret"), "secret leaked: {json}");
    }

    #[test]
    fn scrub_incoming_profile_clears_secrets_and_flags() {
        let mut profile = make_profile("p1", Some(make_ssh(None)));
        profile.request.mysql.password = "pw".to_owned();
        profile.clear_password = true;
        profile.clear_ssh_passphrase = true;
        profile.clear_ssh_password = true;
        profile.request.mysql.file_path = Some("C:\\Users\\alice\\app.db".to_owned());
        {
            let ssh = profile.request.ssh.as_mut().unwrap();
            ssh.passphrase = "pp".to_owned();
            ssh.ssh_password = "sp".to_owned();
        }

        scrub_incoming_profile(&mut profile);

        assert!(profile.request.mysql.password.is_empty());
        // Another machine's SQLite file path (#97), like the other machine-local paths.
        assert!(profile.request.mysql.file_path.is_none());
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
        local.request.mysql.tls_ca_cert_path = Some("/home/local/ca.pem".to_owned());
        local.request.mysql.file_path = Some("/home/local/app.db".to_owned());
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
        assert_eq!(
            incoming.request.mysql.file_path.as_deref(),
            Some("/home/local/app.db")
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
        incoming.request.mysql.tls_ca_cert_path = Some("/home/other/ca.pem".to_owned());
        incoming.request.mysql.file_path = Some("/home/other/app.db".to_owned());

        carry_over_machine_local_paths(&local, &mut incoming);

        assert!(incoming.request.ssh.unwrap().private_key_path.is_none());
        assert!(incoming.request.mysql.tls_ca_cert_path.is_none());
        assert!(incoming.request.mysql.file_path.is_none());
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
        assert_eq!(it.request.mysql.ssl_mode, SslMode::VerifyCa);
    }

    // Machine A exports, machine B imports: B must keep its own key/cert paths while
    // picking up every other change A made. This is the regression the issue is about.
    #[test]
    fn sync_roundtrip_preserves_local_paths_and_applies_remote_edits() {
        let mut on_machine_a =
            make_profile("p1", Some(make_ssh(Some("C:\\Users\\alice\\id_ed25519"))));
        on_machine_a.name = "renamed on A".to_owned();
        on_machine_a.request.mysql.host = "db.example.com".to_owned();
        on_machine_a.request.mysql.tls_ca_cert_path = Some("C:\\Users\\alice\\ca.pem".to_owned());
        let store_a = ConnectionProfileStore {
            version: 1,
            app_version: None,
            groups: vec![],
            items: vec![on_machine_a],
        };

        // A writes the sync file; B reads it back.
        let json = sanitized_store_json(&store_a).unwrap();
        let incoming: ConnectionProfileStore = serde_json::from_str(&json).unwrap();

        let mut on_machine_b =
            make_profile("p1", Some(make_ssh(Some("/home/bob/.ssh/id_ed25519"))));
        on_machine_b.request.mysql.tls_ca_cert_path = Some("/home/bob/ca.pem".to_owned());

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

    // ── Merging against the baseline (#143) ──

    fn store_of(items: Vec<ConnectionProfile>) -> ConnectionProfileStore {
        ConnectionProfileStore {
            version: 2,
            app_version: None,
            groups: vec![],
            items,
        }
    }

    // What another process reads back after `store` was written to the sync file.
    fn as_synced(store: &ConnectionProfileStore) -> ConnectionProfileStore {
        parse_sync_store(&sanitized_store_json(store).unwrap()).unwrap()
    }

    fn renamed(id: &str, name: &str) -> ConnectionProfile {
        let mut p = make_profile(id, None);
        name.clone_into(&mut p.name);
        p
    }

    fn names(store: &ConnectionProfileStore) -> Vec<(&str, &str)> {
        store
            .items
            .iter()
            .map(|p| (p.id.as_str(), p.name.as_str()))
            .collect()
    }

    // A save on this machine must not undo what another machine wrote meanwhile, and the
    // other machine's write must not undo the save.
    #[test]
    fn write_keeps_the_local_edit_and_takes_remote_edits_to_others() {
        let baseline = as_synced(&store_of(vec![renamed("p1", "a"), renamed("p2", "b")]));
        let remote = as_synced(&store_of(vec![renamed("p1", "a"), renamed("p2", "b2")]));
        let mut local = store_of(vec![renamed("p1", "a-local"), renamed("p2", "b")]);

        merge_sync(&mut local, &remote, Some(&baseline), true);

        assert_eq!(names(&local), vec![("p1", "a-local"), ("p2", "b2")]);
    }

    #[test]
    fn write_keeps_the_local_edit_when_both_changed_it() {
        let baseline = as_synced(&store_of(vec![renamed("p1", "a")]));
        let remote = as_synced(&store_of(vec![renamed("p1", "a-remote")]));
        let mut local = store_of(vec![renamed("p1", "a-local")]);

        merge_sync(&mut local, &remote, Some(&baseline), true);

        assert_eq!(names(&local), vec![("p1", "a-local")]);
    }

    #[test]
    fn import_takes_remote_edits_and_keeps_unsent_local_ones() {
        let baseline = as_synced(&store_of(vec![renamed("p1", "a"), renamed("p2", "b")]));
        let remote = as_synced(&store_of(vec![
            renamed("p1", "a"),
            renamed("p2", "b-remote"),
        ]));
        // p1 changed here only (say the mirror failed offline); p2 changed on both sides.
        let mut local = store_of(vec![renamed("p1", "a-local"), renamed("p2", "b-local")]);

        merge_sync(&mut local, &remote, Some(&baseline), false);

        assert_eq!(names(&local), vec![("p1", "a-local"), ("p2", "b-remote")]);
    }

    #[test]
    fn local_deletion_stays_unless_the_profile_changed_remotely() {
        let baseline = as_synced(&store_of(vec![renamed("p1", "a"), renamed("p2", "b")]));
        let remote = as_synced(&store_of(vec![
            renamed("p1", "a"),
            renamed("p2", "b-remote"),
            renamed("p3", "new"),
        ]));
        let mut local = store_of(vec![]);

        merge_sync(&mut local, &remote, Some(&baseline), true);

        assert_eq!(names(&local), vec![("p2", "b-remote"), ("p3", "new")]);
    }

    #[test]
    fn without_a_baseline_import_takes_the_file_and_write_keeps_local() {
        let remote = as_synced(&store_of(vec![
            renamed("p1", "remote"),
            renamed("p2", "new"),
        ]));

        let mut imported = store_of(vec![renamed("p1", "local")]);
        merge_sync(&mut imported, &remote, None, false);
        assert_eq!(names(&imported), vec![("p1", "remote"), ("p2", "new")]);

        let mut written = store_of(vec![renamed("p1", "local")]);
        merge_sync(&mut written, &remote, None, true);
        assert_eq!(names(&written), vec![("p1", "local"), ("p2", "new")]);
    }

    // The baseline holds the file's form (paths stripped), the local store this machine's
    // paths. That difference alone must not count as a local edit.
    #[test]
    fn machine_local_paths_are_not_a_local_change() {
        let mut p1 = make_profile("p1", Some(make_ssh(Some("C:\\Users\\me\\id_ed25519"))));
        p1.request.mysql.tls_ca_cert_path = Some("C:\\Users\\me\\ca.pem".to_owned());
        let baseline = as_synced(&store_of(vec![p1.clone()]));
        let mut remote_p1 = p1.clone();
        "remote".clone_into(&mut remote_p1.name);
        let remote = as_synced(&store_of(vec![remote_p1]));
        let mut local = store_of(vec![p1]);

        merge_sync(&mut local, &remote, Some(&baseline), true);

        let merged = &local.items[0];
        assert_eq!(merged.name, "remote");
        assert_eq!(
            merged
                .request
                .ssh
                .as_ref()
                .unwrap()
                .private_key_path
                .as_deref(),
            Some("C:\\Users\\me\\id_ed25519")
        );
        assert_eq!(
            merged.request.mysql.tls_ca_cert_path.as_deref(),
            Some("C:\\Users\\me\\ca.pem")
        );
    }

    // ── App-version write guard (#81) ──

    #[test]
    fn sanitized_store_json_stamps_current_app_version() {
        let store = ConnectionProfileStore {
            version: 2,
            app_version: None,
            groups: vec![],
            items: vec![],
        };
        let parsed: ConnectionProfileStore =
            serde_json::from_str(&sanitized_store_json(&store).unwrap()).unwrap();
        assert_eq!(parsed.app_version.as_deref(), Some(APP_VERSION));
    }

    // Whether a write to the sync file at `path` is refused because a newer muSQL wrote it.
    fn sync_file_written_by_newer(path: &str) -> bool {
        match read_sync_for_write(path) {
            Err(e) => e == SYNC_BLOCKED_NEWER,
            Ok(_) => false,
        }
    }

    // Write a sync file carrying `app_version` and return whether this build considers it
    // newer. Uses a per-test filename in the temp dir so parallel tests don't collide.
    fn newer_check_with_version(name: &str, version: Option<&str>) -> bool {
        let store = ConnectionProfileStore {
            version: 2,
            app_version: version.map(str::to_owned),
            groups: vec![],
            items: vec![],
        };
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, serde_json::to_string(&store).unwrap()).unwrap();
        let result = sync_file_written_by_newer(path.to_str().unwrap());
        let _ = std::fs::remove_file(&path);
        result
    }

    #[test]
    fn sync_guard_blocks_strictly_newer_file() {
        assert!(newer_check_with_version(
            "musql_guard_newer.json",
            Some("999.0.0"),
        ));
    }

    #[test]
    fn sync_guard_allows_equal_and_older_and_absent() {
        // Same version — safe to write.
        assert!(!newer_check_with_version(
            "musql_guard_equal.json",
            Some(APP_VERSION),
        ));
        // Older writer — safe to write.
        assert!(!newer_check_with_version(
            "musql_guard_older.json",
            Some("0.0.1"),
        ));
        // No stamp (pre-#81 file or the local store) — writing was always allowed here.
        assert!(!newer_check_with_version("musql_guard_absent.json", None));
    }

    #[test]
    fn sync_guard_allows_when_unreadable_or_malformed() {
        // Missing file — falls through to "allowed".
        let missing = std::env::temp_dir().join("musql_guard_does_not_exist.json");
        let _ = std::fs::remove_file(&missing);
        assert!(!sync_file_written_by_newer(missing.to_str().unwrap()));

        // Malformed JSON — allowed rather than blocking the user out.
        let bad = std::env::temp_dir().join("musql_guard_malformed.json");
        std::fs::write(&bad, "not json {").unwrap();
        assert!(!sync_file_written_by_newer(bad.to_str().unwrap()));
        let _ = std::fs::remove_file(&bad);
    }

    #[test]
    fn sync_guard_allows_when_version_unparseable() {
        // A garbage version string must not block writes (it can't be proven newer).
        assert!(!newer_check_with_version(
            "musql_guard_badver.json",
            Some("not-a-semver"),
        ));
    }
}
