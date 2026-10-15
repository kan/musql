//! Redis connections (#98).
//!
//! A Redis profile keeps its server under the same `mysql` key as every other engine
//! (#132): `username` is the ACL user (empty for the default user), `database` is the DB
//! number, and `ssl_mode` / `tls_ca_cert_path` choose the TLS settings. The SSH tunnel and
//! the credential lookup are shared with MySQL; only the last hop differs.

use crate::{unknown_ssl_mode_error, MySqlConfig, SslMode, CONNECT_TIMEOUT};
use redis::{ConnectionAddr, IntoConnectionInfo, RedisConnectionInfo};
// For `write!` into a `String` (which cannot fail).
use std::fmt::Write as _;

/// The DB number a profile selects. Empty means 0, as in redis-cli.
pub fn db_number(database: Option<&str>) -> Result<i64, String> {
    match database.map(str::trim).filter(|d| !d.is_empty()) {
        None => Ok(0),
        Some(d) => d
            .parse::<i64>()
            .ok()
            .filter(|n| *n >= 0)
            .ok_or_else(|| format!("The Redis DB number must be 0 or more, not \"{d}\".")),
    }
}

/// Builds the client for `mysql`'s server, reached at `host:port` (the tunnel's end when
/// SSH is used).
///
/// The SSL modes map onto TLS as for MySQL: `REQUIRED` encrypts without checking the
/// certificate, `VERIFY_CA` checks the certificate but not the host name, and
/// `VERIFY_IDENTITY` checks both. The CA certificate, when given, replaces the system's
/// trusted roots.
pub fn client(mysql: &MySqlConfig, host: &str, port: u16) -> Result<redis::Client, String> {
    let invalid = |e: redis::RedisError| format!("Invalid Redis connection settings: {e}");
    // (use TLS, check the certificate, check the host name)
    let (tls, verify, check_host) = match &mysql.ssl_mode {
        SslMode::Disabled => (false, false, false),
        SslMode::Required => (true, false, false),
        SslMode::VerifyCa => (true, true, false),
        SslMode::VerifyIdentity => (true, true, true),
        SslMode::Unknown(name) => return Err(unknown_ssl_mode_error(name)),
    };

    let mut addr = if tls {
        ConnectionAddr::TcpTls {
            host: host.to_owned(),
            port,
            insecure: !verify,
            tls_params: None,
        }
    } else {
        ConnectionAddr::Tcp(host.to_owned(), port)
    };
    if verify && !check_host {
        addr.set_danger_accept_invalid_hostnames(true);
    }
    let mut settings = RedisConnectionInfo::default().set_db(db_number(mysql.database.as_deref())?);
    if !mysql.username.trim().is_empty() {
        settings = settings.set_username(mysql.username.trim());
    }
    if !mysql.password.is_empty() {
        settings = settings.set_password(&mysql.password);
    }
    let info = addr
        .into_connection_info()
        .map_err(invalid)?
        .set_redis_settings(settings);

    // A CA certificate is only used when the certificate is checked.
    let Some(ca_path) = mysql.ca_cert_path().filter(|_| verify) else {
        return redis::Client::open(info).map_err(invalid);
    };
    let pem =
        std::fs::read(ca_path).map_err(|e| format!("Failed to read the CA certificate: {e}"))?;
    let client = redis::Client::build_with_tls(
        info,
        redis::TlsCertificates {
            client_tls: None,
            root_cert: Some(pem),
        },
    )
    .map_err(invalid)?;
    if check_host {
        return Ok(client);
    }
    // `build_with_tls` replaces the TLS parameters, which drops the host-name setting.
    let mut addr = client.get_connection_info().addr().clone();
    addr.set_danger_accept_invalid_hostnames(true);
    redis::Client::open(client.get_connection_info().clone().set_addr(addr)).map_err(invalid)
}

/// Opens a connection and checks that the server answers.
pub fn ping(client: &redis::Client) -> Result<(), String> {
    let mut conn = connect(client)?;
    redis::cmd("PING")
        .query::<String>(&mut conn)
        .map(|_| ())
        .map_err(|e| format!("Redis did not answer PING: {e}"))
}

pub fn connect(client: &redis::Client) -> Result<redis::Connection, String> {
    let conn = client
        .get_connection_with_timeout(CONNECT_TIMEOUT)
        .map_err(|e| format!("Failed to connect Redis: {e}"))?;
    conn.set_read_timeout(Some(CONNECT_TIMEOUT))
        .and_then(|()| conn.set_write_timeout(Some(CONNECT_TIMEOUT)))
        .map_err(|e| format!("Failed to set up the Redis connection: {e}"))?;
    Ok(conn)
}

// ── The Redis window's session ──

/// How many keys one SCAN step asks for, and about how many keys one `scan` returns.
const SCAN_COUNT: usize = 200;
/// The SCAN steps one `scan` takes at most. A step can return few or no matches for a
/// narrow pattern, so one step alone would often show an empty list.
const SCAN_STEPS: usize = 50;
/// The items of one key shown at most, as for query results (`Limits` in frontend.md).
const ROW_LIMIT: usize = 500;
/// The part of a string value shown at most.
const STRING_LIMIT: usize = 1024 * 1024;

/// Commands that act on the whole server, block it while they run (KEYS, SAVE), or change
/// who can connect (CLIENT KILL, ACL SETUSER / DELUSER). The CLI asks before running them
/// (#98). Matched on the arguments as the server sees them, so quoting the
/// name does not get around the check.
const DANGEROUS_COMMANDS: &[&str] = &[
    "FLUSHALL",
    "FLUSHDB",
    "SHUTDOWN",
    "DEBUG",
    "SWAPDB",
    "KEYS",
    "REPLICAOF",
    "SLAVEOF",
    "FAILOVER",
    "SAVE",
];
/// Commands that are dangerous only with some subcommands.
const DANGEROUS_SUBCOMMANDS: &[(&str, &[&str])] = &[
    ("CONFIG", &["SET", "REWRITE", "RESETSTAT"]),
    ("SCRIPT", &["FLUSH"]),
    ("FUNCTION", &["FLUSH", "DELETE", "RESTORE"]),
    ("CLUSTER", &["RESET", "FAILOVER", "FORGET"]),
    ("CLIENT", &["PAUSE", "KILL"]),
    ("ACL", &["SETUSER", "DELUSER"]),
];

/// Commands that turn the connection into a stream of pushed messages. The CLI tab sends
/// one command and reads one reply, so these would leave the connection unusable.
const STREAMING_COMMANDS: &[&str] = &[
    "SUBSCRIBE",
    "PSUBSCRIBE",
    "SSUBSCRIBE",
    "MONITOR",
    "SYNC",
    "PSYNC",
];

/// Commands that change the connection's state behind the session's back. The CLI shares
/// the connection with the key list and the key tabs, which would then break: after MULTI
/// every call gets `QUEUED`, after RESET or HELLO the DB or the protocol is not what the
/// session knows, after AUTH the list runs as another user (until a reconnect quietly
/// goes back to the profile's), after QUIT the next call fails, and after CLIENT REPLY OFF
/// no call gets an answer.
const STATEFUL_COMMANDS: &[&str] = &["MULTI", "WATCH", "RESET", "HELLO", "AUTH", "QUIT"];
const STATEFUL_SUBCOMMANDS: &[(&str, &[&str])] = &[("CLIENT", &["REPLY"])];

/// The number of connects started, and of disconnects. A connect stores its session only
/// when nothing started after it, so one that finishes late (a slow SSH tunnel) does not
/// replace a newer session, or reopen one for a window already closed.
static EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Starts a connect or a disconnect: the earlier connects that are still running will not
/// store their sessions. Returns the new epoch.
pub fn next_epoch() -> u64 {
    EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

/// Whether `epoch` is still the latest connect. Checked under the state's lock.
pub fn is_current(epoch: u64) -> bool {
    current_epoch() == epoch
}

/// The latest connect or disconnect, for work that should stop when either comes.
pub fn current_epoch() -> u64 {
    EPOCH.load(std::sync::atomic::Ordering::SeqCst)
}

/// The number of key listings started. A listing still scanning when a newer one starts
/// (the pattern box changed again) stops at its next SCAN step instead of holding the
/// connection until it is done: the window would drop its keys anyway.
static SCAN_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Starts a listing; returns its number for `scan`'s `still_wanted`.
pub fn next_scan_epoch() -> u64 {
    SCAN_EPOCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

pub fn is_current_scan(epoch: u64) -> bool {
    SCAN_EPOCH.load(std::sync::atomic::Ordering::SeqCst) == epoch
}

/// Locks the state. A panic while the lock was held leaves the session in an unknown
/// state, so it is dropped (the next call reports "Not connected"), and the lock is used
/// again rather than failing for the rest of the run.
pub fn lock(state: &RedisState) -> std::sync::MutexGuard<'_, Option<RedisSession>> {
    state.lock().unwrap_or_else(|poisoned| {
        state.clear_poison();
        let mut guard = poisoned.into_inner();
        *guard = None;
        guard
    })
}

/// The Redis window's open connection: one at a time, like the query window's pool.
pub struct RedisSession {
    client: redis::Client,
    /// `None` after an I/O error; the next call connects again.
    conn: Option<redis::Connection>,
    /// The DB in use: the profile's, until the CLI runs SELECT. A new connection selects
    /// it again, so a reconnect does not quietly go back to the profile's DB.
    db: i64,
    /// The SSH tunnel, kept open for as long as the session uses it.
    _tunnel: Option<crate::SshTunnel>,
}

pub type RedisState = std::sync::Arc<std::sync::Mutex<Option<RedisSession>>>;

impl RedisSession {
    pub fn open(client: redis::Client, tunnel: Option<crate::SshTunnel>) -> Result<Self, String> {
        let conn = connect(&client)?;
        let db = client.get_connection_info().redis_settings().db();
        Ok(Self {
            client,
            conn: Some(conn),
            db,
            _tunnel: tunnel,
        })
    }

    pub fn db(&self) -> i64 {
        self.db
    }

    /// Switches to `db` (the DB switcher). The CLI's SELECT ends in the same place:
    /// `command` records the DB once the server accepted it.
    pub fn select(&mut self, db: i64) -> Result<(), String> {
        self.with_conn(|c| redis::cmd("SELECT").arg(db).query::<()>(c))?
            .map_err(|e| server_error(&e))?;
        self.db = db;
        Ok(())
    }

    /// Runs `f` on the connection. A timeout or a dropped socket leaves the connection
    /// in an unknown state (a reply may still be on its way), so it is closed and the
    /// next call opens a new one.
    fn with_conn<T>(
        &mut self,
        f: impl FnOnce(&mut redis::Connection) -> redis::RedisResult<T>,
    ) -> Result<redis::RedisResult<T>, String> {
        let conn = match &mut self.conn {
            Some(conn) => conn,
            None => {
                let mut conn = connect(&self.client)?;
                redis::cmd("SELECT")
                    .arg(self.db)
                    .query::<()>(&mut conn)
                    .map_err(|e| format!("Failed to select Redis DB {}: {e}", self.db))?;
                self.conn.insert(conn)
            }
        };
        let result = f(conn);
        if let Err(e) = &result {
            if is_connection_error(e) {
                self.conn = None;
            }
        }
        Ok(result)
    }
}

fn is_connection_error(e: &redis::RedisError) -> bool {
    e.is_io_error() || e.is_timeout() || e.is_connection_dropped() || e.is_unrecoverable_error()
}

/// The server's version, from `INFO server`.
pub fn server_version(session: &mut RedisSession) -> Result<String, String> {
    let info: String = session
        .with_conn(|c| redis::cmd("INFO").arg("server").query(c))?
        .map_err(|e| e.to_string())?;
    Ok(info
        .lines()
        .find_map(|line| line.strip_prefix("redis_version:"))
        .unwrap_or("")
        .trim()
        .to_owned())
}

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct DbInfo {
    db: i64,
    /// The number of keys, from `INFO keyspace` (0 for a DB it does not list).
    keys: u64,
}

/// The server's DBs with their key counts, for the DB switcher. The count comes from
/// `CONFIG GET databases`, which ACL can deny: then 16 (Redis's default), or more when
/// `INFO keyspace` lists a higher DB.
pub fn databases(session: &mut RedisSession) -> Result<Vec<DbInfo>, String> {
    let (configured, keyspace) = session
        .with_conn(|c| {
            let configured = redis::cmd("CONFIG")
                .arg("GET")
                .arg("databases")
                .query::<Vec<String>>(c)
                .ok()
                .and_then(|reply| reply.get(1).and_then(|n| n.parse::<i64>().ok()));
            let keyspace: String = redis::cmd("INFO").arg("keyspace").query(c)?;
            Ok((configured, keyspace))
        })?
        .map_err(|e| e.to_string())?;
    Ok(db_list(configured, &keyspace))
}

/// `INFO keyspace` (`db0:keys=309,expires=1,...`) over `count` DBs (16 when unknown).
fn db_list(count: Option<i64>, keyspace: &str) -> Vec<DbInfo> {
    let listed: std::collections::BTreeMap<i64, u64> = keyspace
        .lines()
        .filter_map(|line| {
            let (db, fields) = line.strip_prefix("db")?.split_once(':')?;
            let keys = fields
                .split(',')
                .find_map(|f| f.strip_prefix("keys="))?
                .trim()
                .parse()
                .ok()?;
            Some((db.parse().ok()?, keys))
        })
        .collect();
    let highest = listed.last_key_value().map_or(0, |(db, _)| db + 1);
    let count = count.unwrap_or(16).max(highest);
    (0..count)
        .map(|db| DbInfo {
            db,
            keys: listed.get(&db).copied().unwrap_or(0),
        })
        .collect()
}

/// One command for the CLI's completion and help.
#[derive(serde::Serialize, Debug, PartialEq, Default)]
pub struct CommandDoc {
    /// Upper case; a subcommand with its container (`CONFIG GET`).
    name: String,
    /// `string`, `hash`, ... (empty when the server does not say).
    group: String,
    summary: String,
    /// The arguments in the documentation's notation: `key field value [field value ...]`.
    syntax: String,
}

/// The server's commands from `COMMAND DOCS` (Redis 7 and later). An older server, or one
/// whose ACL denies it, gives the names alone from `COMMAND`, or nothing.
pub fn command_docs(session: &mut RedisSession) -> Result<Vec<CommandDoc>, String> {
    let mut out = Vec::new();
    if let Ok(value) =
        session.with_conn(|c| redis::cmd("COMMAND").arg("DOCS").query::<redis::Value>(c))?
    {
        collect_docs(&value, &mut out);
    }
    if out.is_empty() {
        if let Ok(redis::Value::Array(commands)) =
            session.with_conn(|c| redis::cmd("COMMAND").query::<redis::Value>(c))?
        {
            out = commands
                .iter()
                .filter_map(|command| match command {
                    redis::Value::Array(info) => info.first().map(plain),
                    _ => None,
                })
                .map(|name| CommandDoc {
                    name: name.to_ascii_uppercase(),
                    ..CommandDoc::default()
                })
                .collect();
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// A reply's map as (key, value) pairs: a RESP3 map, or a RESP2 array of alternating
/// keys and values.
fn pairs(value: &redis::Value) -> Vec<(String, &redis::Value)> {
    match value {
        redis::Value::Map(map) => map.iter().map(|(k, v)| (plain(k), v)).collect(),
        redis::Value::Array(items) => items
            .chunks(2)
            .filter_map(|pair| match pair {
                [k, v] => Some((plain(k), v)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn field<'a>(map: &'a [(String, &'a redis::Value)], name: &str) -> Option<&'a redis::Value> {
    map.iter().find(|(k, _)| k == name).map(|(_, v)| *v)
}

/// COMMAND DOCS's reply (name, doc, name, doc, ...), subcommands included.
fn collect_docs(value: &redis::Value, out: &mut Vec<CommandDoc>) {
    for (name, doc) in pairs(value) {
        let doc = pairs(doc);
        let text = |key: &str| field(&doc, key).map(plain).unwrap_or_default();
        out.push(CommandDoc {
            // `config|get` → `CONFIG GET`
            name: name.replace('|', " ").to_ascii_uppercase(),
            group: text("group"),
            summary: text("summary"),
            syntax: arg_syntaxes(&doc).join(" "),
        });
        if let Some(subcommands) = field(&doc, "subcommands") {
            collect_docs(subcommands, out);
        }
    }
}

/// The syntax of each argument a command's (or a block's) `arguments` lists.
fn arg_syntaxes(map: &[(String, &redis::Value)]) -> Vec<String> {
    match field(map, "arguments") {
        Some(redis::Value::Array(args)) => args.iter().map(arg_syntax).collect(),
        _ => Vec::new(),
    }
}

/// One argument in the documentation's notation: `[x]` for optional, `x [x ...]` for
/// repeated, `<a | b>` for a choice, and its token before it (`EX seconds`).
fn arg_syntax(arg: &redis::Value) -> String {
    let arg = pairs(arg);
    let text = |key: &str| field(&arg, key).map(plain);
    let kind = text("type").unwrap_or_default();
    let flags: Vec<String> = match field(&arg, "flags") {
        Some(redis::Value::Array(flags) | redis::Value::Set(flags)) => {
            flags.iter().map(plain).collect()
        }
        _ => Vec::new(),
    };
    let has = |flag: &str| flags.iter().any(|f| f == flag);
    let mut s = match kind.as_str() {
        "pure-token" => text("token").or_else(|| text("name")).unwrap_or_default(),
        "oneof" => {
            let choice = arg_syntaxes(&arg).join(" | ");
            if has("optional") {
                choice
            } else {
                format!("<{choice}>")
            }
        }
        "block" => arg_syntaxes(&arg).join(" "),
        _ => text("display_text")
            .or_else(|| text("name"))
            .unwrap_or_default(),
    };
    if kind != "pure-token" {
        if let Some(token) = text("token") {
            s = format!("{token} {s}");
        }
    }
    if has("multiple") || has("multiple_token") {
        s = format!("{s} [{s} ...]");
    }
    if has("optional") {
        s = format!("[{s}]");
    }
    s
}

#[derive(serde::Serialize)]
pub struct KeyEntry {
    /// The name to show: the key's UTF-8, with any other byte written as `\xHH`.
    key: String,
    /// The key's bytes in hex, when they are not valid UTF-8; the window sends it back to
    /// open the key (`key` alone would name another key).
    raw: Option<String>,
    kind: String,
}

impl KeyEntry {
    fn new(bytes: Vec<u8>, kind: String) -> Self {
        match String::from_utf8(bytes) {
            Ok(key) => Self {
                key,
                raw: None,
                kind,
            },
            Err(e) => {
                let bytes = e.into_bytes();
                Self {
                    key: escaped(&bytes),
                    raw: Some(bytes.iter().map(|b| format!("{b:02x}")).collect()),
                    kind,
                }
            }
        }
    }
}

/// Bytes as text, with the bytes that are not valid UTF-8 written as `\xHH` (as redis-cli
/// writes them, so the name can be typed in the CLI inside "...").
fn escaped(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.utf8_chunks() {
        out.push_str(chunk.valid());
        for b in chunk.invalid() {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out
}

/// A key the window names: its `raw` hex when it has one, else its text.
pub fn key_bytes(key: &str, raw: Option<&str>) -> Result<Vec<u8>, String> {
    let Some(hex) = raw else {
        return Ok(key.as_bytes().to_vec());
    };
    if hex.len() % 2 != 0 {
        return Err("Invalid key bytes.".to_owned());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| "Invalid key bytes.".to_owned())
        })
        .collect()
}

#[derive(serde::Serialize)]
pub struct ScanPage {
    /// "0" when the scan is complete. A string: some servers (Dragonfly, proxies that put
    /// a shard in the high bits) return cursors above 2^53, which a JS number rounds.
    cursor: String,
    keys: Vec<KeyEntry>,
}

/// The next keys matching `pattern` (all keys when it is empty), with each key's type:
/// SCAN steps from `cursor` until about `SCAN_COUNT` keys arrived, the scan ended, or
/// `SCAN_STEPS` steps were taken. The types come in one pipeline at the end. Stops early
/// (with what it has) once `still_wanted` says a newer listing started.
pub fn scan(
    session: &mut RedisSession,
    cursor: u64,
    pattern: &str,
    still_wanted: impl Fn() -> bool,
) -> Result<ScanPage, String> {
    let pattern = if pattern.trim().is_empty() {
        "*"
    } else {
        pattern
    };
    session
        .with_conn(|c| {
            let mut cursor = cursor;
            let mut keys: Vec<Vec<u8>> = Vec::new();
            for _ in 0..SCAN_STEPS {
                let (next, step): (u64, Vec<Vec<u8>>) = redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(pattern)
                    .arg("COUNT")
                    .arg(SCAN_COUNT)
                    .query(c)?;
                keys.extend(step);
                cursor = next;
                if cursor == 0 || keys.len() >= SCAN_COUNT || !still_wanted() {
                    break;
                }
            }
            let kinds: Vec<String> = if keys.is_empty() {
                Vec::new()
            } else {
                let mut pipe = redis::pipe();
                for key in &keys {
                    pipe.cmd("TYPE").arg(key);
                }
                pipe.query(c)?
            };
            Ok(ScanPage {
                cursor: cursor.to_string(),
                keys: keys
                    .into_iter()
                    .zip(kinds)
                    .map(|(key, kind)| KeyEntry::new(key, kind))
                    .collect(),
            })
        })?
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize, Default)]
pub struct KeyView {
    /// The Redis type (`string`, `hash`, ...); `none` when the key is gone.
    kind: String,
    /// Seconds left, -1 without an expiry.
    ttl: i64,
    /// Bytes, when the server reports it (`MEMORY USAGE` can be denied by ACL).
    memory: Option<i64>,
    /// The length of the string, or the number of items.
    size: i64,
    /// A string value.
    text: Option<String>,
    /// Any other type, as a table.
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    /// More than is shown (`ROW_LIMIT` items, `STRING_LIMIT` bytes).
    truncated: bool,
}

/// What the key holds, for the key's tab.
pub fn key_view(session: &mut RedisSession, key: &[u8]) -> Result<KeyView, String> {
    session
        .with_conn(|c| {
            // The header in one round trip. MEMORY USAGE can be denied by ACL, so the
            // replies are taken one by one.
            let mut header: Vec<redis::RedisResult<redis::Value>> = redis::pipe()
                .ignore_errors()
                .cmd("TYPE")
                .arg(key)
                .cmd("TTL")
                .arg(key)
                .cmd("MEMORY")
                .arg("USAGE")
                .arg(key)
                .query(c)?;
            let memory = header
                .pop()
                .and_then(|r| r.ok())
                .and_then(|v| redis::from_redis_value::<Option<i64>>(v).ok())
                .flatten();
            let ttl: i64 = redis::from_redis_value(header.pop().unwrap_or(Ok(redis::Value::Nil))?)?;
            let kind: String =
                redis::from_redis_value(header.pop().unwrap_or(Ok(redis::Value::Nil))?)?;
            if kind == "none" {
                return Ok(KeyView {
                    kind,
                    ..KeyView::default()
                });
            }
            let mut view = KeyView {
                kind,
                ttl,
                memory,
                ..KeyView::default()
            };
            match view.kind.as_str() {
                "string" => {
                    view.size = redis::cmd("STRLEN").arg(key).query(c)?;
                    let mut bytes: Vec<u8> = redis::cmd("GETRANGE")
                        .arg(key)
                        .arg(0)
                        .arg(STRING_LIMIT - 1)
                        .query(c)?;
                    view.truncated = view.size > STRING_LIMIT as i64;
                    if view.truncated {
                        trim_cut_character(&mut bytes);
                    }
                    view.text = Some(text(bytes));
                    return Ok(view);
                }
                "hash" => {
                    view.size = redis::cmd("HLEN").arg(key).query(c)?;
                    view.columns = vec!["field".to_owned(), "value".to_owned()];
                    view.rows = scan_items(c, "HSCAN", key, 2)?;
                }
                "list" => {
                    view.size = redis::cmd("LLEN").arg(key).query(c)?;
                    let items: Vec<Vec<u8>> = redis::cmd("LRANGE")
                        .arg(key)
                        .arg(0)
                        .arg(ROW_LIMIT - 1)
                        .query(c)?;
                    view.columns = vec!["index".to_owned(), "value".to_owned()];
                    view.rows = items
                        .into_iter()
                        .enumerate()
                        .map(|(i, item)| vec![i.to_string(), text(item)])
                        .collect();
                }
                "set" => {
                    view.size = redis::cmd("SCARD").arg(key).query(c)?;
                    view.columns = vec!["member".to_owned()];
                    view.rows = scan_items(c, "SSCAN", key, 1)?;
                }
                "zset" => {
                    view.size = redis::cmd("ZCARD").arg(key).query(c)?;
                    let items: Vec<(Vec<u8>, String)> = redis::cmd("ZRANGE")
                        .arg(key)
                        .arg(0)
                        .arg(ROW_LIMIT - 1)
                        .arg("WITHSCORES")
                        .query(c)?;
                    view.columns = vec!["score".to_owned(), "member".to_owned()];
                    view.rows = items
                        .into_iter()
                        .map(|(member, score)| vec![score, text(member)])
                        .collect();
                }
                "stream" => {
                    view.size = redis::cmd("XLEN").arg(key).query(c)?;
                    let entries: redis::Value = redis::cmd("XRANGE")
                        .arg(key)
                        .arg("-")
                        .arg("+")
                        .arg("COUNT")
                        .arg(ROW_LIMIT)
                        .query(c)?;
                    view.columns = vec!["id".to_owned(), "fields".to_owned()];
                    view.rows = stream_rows(&entries);
                }
                // A module's type (JSON, ...): the header only.
                _ => return Ok(view),
            }
            view.truncated = view.size > view.rows.len() as i64;
            Ok(view)
        })?
        .map_err(|e| e.to_string())
}

/// The items of a hash (`width` 2) or a set (`width` 1) through HSCAN / SSCAN, up to
/// `ROW_LIMIT`. Each step asks for `ROW_LIMIT`, so a small key takes one round trip.
/// SCAN may return an item twice; that is accepted for a view.
fn scan_items(
    c: &mut redis::Connection,
    command: &str,
    key: &[u8],
    width: usize,
) -> redis::RedisResult<Vec<Vec<String>>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cursor: u64 = 0;
    loop {
        let (next, items): (u64, Vec<Vec<u8>>) = redis::cmd(command)
            .arg(key)
            .arg(cursor)
            .arg("COUNT")
            .arg(ROW_LIMIT)
            .query(c)?;
        let mut items = items.into_iter();
        while let Some(first) = items.next() {
            let mut row = vec![text(first)];
            row.extend(items.by_ref().take(width - 1).map(text));
            rows.push(row);
        }
        cursor = next;
        if cursor == 0 || rows.len() >= ROW_LIMIT {
            rows.truncate(ROW_LIMIT);
            return Ok(rows);
        }
    }
}

/// XRANGE's reply as rows of (id, fields as a JSON object), so the row detail formats
/// the fields.
fn stream_rows(entries: &redis::Value) -> Vec<Vec<String>> {
    let redis::Value::Array(entries) = entries else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let redis::Value::Array(parts) = entry else {
                return None;
            };
            let id = parts.first().map(plain).unwrap_or_default();
            let mut fields = serde_json::Map::new();
            if let Some(entry_fields) = parts.get(1) {
                for (name, value) in pairs(entry_fields) {
                    fields.insert(name, serde_json::Value::String(plain(value)));
                }
            }
            Some(vec![id, serde_json::Value::Object(fields).to_string()])
        })
        .collect()
}

/// A reply value as text, without quotes (for table cells).
fn plain(value: &redis::Value) -> String {
    match value {
        redis::Value::BulkString(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        redis::Value::SimpleString(s) => s.clone(),
        redis::Value::Int(n) => n.to_string(),
        other => format_reply(other),
    }
}

/// Drops the end of a UTF-8 character that the `STRING_LIMIT` cut split, so the shown
/// text does not end in U+FFFD. Bytes that are not UTF-8 at all are left as they are.
fn trim_cut_character(bytes: &mut Vec<u8>) {
    // A UTF-8 character is at most 4 bytes, so a cut one starts in the last 3.
    for start in bytes.len().saturating_sub(3)..bytes.len() {
        // `error_len` is None only for a sequence cut off by the end of the input.
        if let Err(e) = std::str::from_utf8(&bytes[start..]) {
            if e.valid_up_to() == 0 && e.error_len().is_none() {
                bytes.truncate(start);
                return;
            }
        }
    }
}

/// Bytes shown as text. Redis values are bytes; invalid UTF-8 becomes U+FFFD. Valid
/// UTF-8 (the usual case) is taken over without a copy.
fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// What one CLI line did.
#[derive(serde::Serialize)]
pub struct CommandReply {
    /// The reply in redis-cli's layout; empty when the command was not run.
    pub reply: String,
    /// The DB in use afterwards (SELECT changes it).
    pub db: i64,
    /// Set when the command needs the user's confirmation and was not run: the command
    /// to name in the question (`FLUSHALL`, `CONFIG SET`).
    pub confirm: Option<String>,
}

/// Runs one CLI line and formats the reply as redis-cli does. A server error (a wrong
/// type, an unknown command) is part of the output, `(error) ...`; only a failure to
/// reach the server is an `Err`. A dangerous command runs only when `confirmed`.
pub fn command(
    session: &mut RedisSession,
    line: &str,
    confirmed: bool,
) -> Result<CommandReply, String> {
    let args = split_command(line)?;
    let done = |session: &RedisSession, reply: String| CommandReply {
        reply,
        db: session.db,
        confirm: None,
    };
    let Some(name) = args.first() else {
        return Ok(done(session, String::new()));
    };
    let upper = word(name);
    if STREAMING_COMMANDS.contains(&upper.as_str()) {
        return Err(format!(
            "{upper} is not supported here: it keeps the connection waiting for messages."
        ));
    }
    if let Some(stateful) = find_command(&args, STATEFUL_COMMANDS, STATEFUL_SUBCOMMANDS) {
        return Err(format!(
            "{stateful} is not supported here: it changes the connection that the key list \
             and the key tabs share."
        ));
    }
    if !confirmed {
        if let Some(confirm) = needs_confirmation(&args) {
            return Ok(CommandReply {
                reply: String::new(),
                db: session.db,
                confirm: Some(confirm),
            });
        }
    }
    let mut cmd = redis::cmd(&upper);
    for arg in &args[1..] {
        cmd.arg(arg);
    }
    match session.with_conn(|c| cmd.query::<redis::Value>(c))? {
        Ok(value) => {
            if upper == "SELECT" {
                if let Some(db) = args.get(1).and_then(|n| word(n).parse().ok()) {
                    session.db = db;
                }
            }
            Ok(done(session, format_reply(&value)))
        }
        Err(e) if is_connection_error(&e) => Err(e.to_string()),
        Err(e) => Ok(done(session, server_error(&e))),
    }
}

/// An argument upper-cased, for matching command names.
fn word(arg: &[u8]) -> String {
    String::from_utf8_lossy(arg).to_ascii_uppercase()
}

/// The command to name when asking before running `args`, or `None` when it needs no
/// confirmation.
fn needs_confirmation(args: &[Vec<u8>]) -> Option<String> {
    find_command(args, DANGEROUS_COMMANDS, DANGEROUS_SUBCOMMANDS)
}

/// `args`'s command (`FLUSHALL`, or `CONFIG SET` with its subcommand) when it is one of
/// `commands`, or one of `subcommands` with its subcommand.
fn find_command(
    args: &[Vec<u8>],
    commands: &[&str],
    subcommands: &[(&str, &[&str])],
) -> Option<String> {
    let name = word(args.first()?);
    if commands.contains(&name.as_str()) {
        return Some(name);
    }
    let sub = word(args.get(1)?);
    subcommands
        .iter()
        .any(|(command, subs)| *command == name && subs.contains(&sub.as_str()))
        .then(|| format!("{name} {sub}"))
}

/// A server's error reply as redis-cli shows it: `(error) WRONGTYPE Operation against ...`.
fn server_error(e: &redis::RedisError) -> String {
    match (e.code(), e.detail()) {
        (Some(code), Some(detail)) => format!("(error) {code} {detail}"),
        (Some(code), None) => format!("(error) {code}"),
        _ => format!("(error) {e}"),
    }
}

/// Splits a CLI line into arguments as redis-cli does: whitespace separates, "..." takes
/// the escapes \" \\ \n \r \t \xHH, and '...' takes only \'. The arguments are bytes:
/// `\xHH` is that one byte, as in redis-cli, so `"\xe3\x81\x82"` is the UTF-8 of あ.
pub fn split_command(line: &str) -> Result<Vec<Vec<u8>>, String> {
    fn push(arg: &mut Vec<u8>, c: char) {
        arg.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
    }
    let mut args = Vec::new();
    let mut chars = line.chars().peekable();
    loop {
        while chars.next_if(|c| c.is_whitespace()).is_some() {}
        let Some(&first) = chars.peek() else {
            return Ok(args);
        };
        let mut arg = Vec::new();
        if first == '"' || first == '\'' {
            chars.next();
            loop {
                match chars.next() {
                    None => return Err("Unbalanced quotes in the command.".to_owned()),
                    Some(c) if c == first => break,
                    Some('\\') if first == '\'' => match chars.next() {
                        Some('\'') => arg.push(b'\''),
                        Some(c) => {
                            arg.push(b'\\');
                            push(&mut arg, c);
                        }
                        None => return Err("Unbalanced quotes in the command.".to_owned()),
                    },
                    Some('\\') => match chars.next() {
                        Some('n') => arg.push(b'\n'),
                        Some('r') => arg.push(b'\r'),
                        Some('t') => arg.push(b'\t'),
                        Some('b') => arg.push(0x08),
                        Some('a') => arg.push(0x07),
                        // \xHH with two hex digits is one byte; otherwise, as in
                        // redis-cli, the escape is just the letter x.
                        Some('x') => {
                            let mut ahead = chars.clone();
                            let digit = |c: Option<char>| c.and_then(|c| c.to_digit(16));
                            match (digit(ahead.next()), digit(ahead.next())) {
                                (Some(high), Some(low)) => {
                                    chars = ahead;
                                    // Two hex digits: at most 0xff.
                                    arg.push((high * 16 + low) as u8);
                                }
                                _ => arg.push(b'x'),
                            }
                        }
                        Some(c) => push(&mut arg, c),
                        None => return Err("Unbalanced quotes in the command.".to_owned()),
                    },
                    Some(c) => push(&mut arg, c),
                }
            }
            // A closing quote must end the argument, as in redis-cli.
            if chars.peek().is_some_and(|c| !c.is_whitespace()) {
                return Err("A closing quote must be followed by a space.".to_owned());
            }
        } else {
            while let Some(c) = chars.next_if(|c| !c.is_whitespace()) {
                push(&mut arg, c);
            }
        }
        args.push(arg);
    }
}

/// A reply in redis-cli's layout: `(nil)`, `(integer) 1`, quoted strings, and numbered
/// lines for arrays (nested ones indented under their number).
pub fn format_reply(value: &redis::Value) -> String {
    let mut out = String::new();
    write_reply(&mut out, value, 0);
    out
}

fn write_reply(out: &mut String, value: &redis::Value, indent: usize) {
    use redis::Value as V;
    // Writing to a String cannot fail.
    let _ = match value {
        V::Nil => write!(out, "(nil)"),
        V::Int(n) => write!(out, "(integer) {n}"),
        V::BulkString(bytes) => write!(out, "{:?}", String::from_utf8_lossy(bytes)),
        V::SimpleString(s) => write!(out, "{s}"),
        V::Okay => write!(out, "OK"),
        V::Double(d) => write!(out, "(double) {d}"),
        V::Boolean(b) => write!(out, "({b})"),
        V::VerbatimString { text, .. } => write!(out, "{text}"),
        V::BigNumber(digits) => write!(out, "(big number) {}", String::from_utf8_lossy(digits)),
        V::ServerError(e) => match e.details() {
            Some(details) => write!(out, "(error) {} {details}", e.code()),
            None => write!(out, "(error) {}", e.code()),
        },
        V::Attribute { data, .. } => {
            write_reply(out, data, indent);
            Ok(())
        }
        V::Array(items) | V::Set(items) | V::Push { data: items, .. } => {
            write_items(out, items.iter().map(|item| (None, item)), indent);
            Ok(())
        }
        V::Map(pairs) => {
            write_items(out, pairs.iter().map(|(k, v)| (Some(k), v)), indent);
            Ok(())
        }
        // A kind of reply a later version of the crate adds.
        other => write!(out, "{other:?}"),
    };
}

fn write_items<'a>(
    out: &mut String,
    items: impl ExactSizeIterator<Item = (Option<&'a redis::Value>, &'a redis::Value)>,
    indent: usize,
) {
    if items.len() == 0 {
        out.push_str("(empty array)");
        return;
    }
    let width = items.len().to_string().len();
    // The number, ") " and the indent: where a nested reply's lines start.
    let inner = indent + width + 2;
    for (i, (key, value)) in items.enumerate() {
        if i > 0 {
            let _ = write!(out, "\n{:indent$}", "");
        }
        let _ = write!(out, "{:>width$}) ", i + 1);
        if let Some(key) = key {
            write_reply(out, key, inner);
            out.push_str(" => ");
        }
        write_reply(out, value, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(ssl_mode: SslMode, database: Option<&str>) -> MySqlConfig {
        MySqlConfig {
            host: "redis.example.com".to_owned(),
            port: 6379,
            database: database.map(str::to_owned),
            username: String::new(),
            password: String::new(),
            ssl_mode,
            tls_ca_cert_path: None,
            save_password: true,
            op_ref: None,
            tls_enabled: false,
            tls_skip_verify: false,
        }
    }

    #[test]
    fn db_number_defaults_to_zero_and_rejects_non_numbers() {
        assert_eq!(db_number(None), Ok(0));
        assert_eq!(db_number(Some(" ")), Ok(0));
        assert_eq!(db_number(Some("3")), Ok(3));
        assert!(db_number(Some("app")).is_err());
        assert!(db_number(Some("-1")).is_err());
    }

    #[test]
    fn ssl_modes_choose_plain_or_tls() {
        let plain = client(&config(SslMode::Disabled, None), "h", 6379).unwrap();
        assert!(matches!(
            plain.get_connection_info().addr(),
            ConnectionAddr::Tcp(..)
        ));

        let unchecked = client(&config(SslMode::Required, None), "h", 6379).unwrap();
        assert!(matches!(
            unchecked.get_connection_info().addr(),
            ConnectionAddr::TcpTls { insecure: true, .. }
        ));

        for mode in [SslMode::VerifyCa, SslMode::VerifyIdentity] {
            let checked = client(&config(mode, None), "h", 6379).unwrap();
            assert!(matches!(
                checked.get_connection_info().addr(),
                ConnectionAddr::TcpTls {
                    insecure: false,
                    ..
                }
            ));
        }

        assert!(client(&config(SslMode::Unknown("X".to_owned()), None), "h", 6379).is_err());
    }

    /// Whether the connection skips the host-name check. The flag sits in a type the
    /// crate keeps private, so it is read off the Debug output.
    fn skips_host_name(client: &redis::Client) -> bool {
        format!("{:?}", client.get_connection_info().addr())
            .contains("danger_accept_invalid_hostnames: true")
    }

    // VERIFY_CA checks the certificate but not the host name; VERIFY_IDENTITY checks
    // both. With a CA certificate, `build_with_tls` replaces the TLS parameters, so that
    // path is checked separately.
    #[test]
    fn verify_ca_alone_skips_the_host_name_with_or_without_a_ca_file() {
        // An empty PEM file parses as no certificates, which is enough to take the path.
        let ca = std::env::temp_dir().join("musql_redis_test_ca.pem");
        std::fs::write(&ca, b"").unwrap();
        for ca_path in [None, Some(ca.to_string_lossy().into_owned())] {
            for (mode, skips) in [(SslMode::VerifyCa, true), (SslMode::VerifyIdentity, false)] {
                let mut mysql = config(mode.clone(), None);
                mysql.tls_ca_cert_path = ca_path.clone();
                let client = client(&mysql, "h", 6379).unwrap();
                assert_eq!(
                    skips_host_name(&client),
                    skips,
                    "{mode:?} with CA {ca_path:?}"
                );
                assert!(matches!(
                    client.get_connection_info().addr(),
                    ConnectionAddr::TcpTls {
                        insecure: false,
                        ..
                    }
                ));
            }
        }
        let _ = std::fs::remove_file(&ca);
    }

    #[test]
    fn credentials_and_db_reach_the_connection() {
        let mut mysql = config(SslMode::Disabled, Some("2"));
        mysql.username = "app".to_owned();
        mysql.password = "secret".to_owned();
        let with_auth = client(&mysql, "h", 6379).unwrap();
        let settings = with_auth.get_connection_info().redis_settings();
        assert_eq!(settings.db(), 2);
        assert_eq!(settings.username(), Some("app"));
        assert_eq!(settings.password(), Some("secret"));

        // Empty user and password mean the default user without AUTH.
        let without = client(&config(SslMode::Disabled, None), "h", 6379).unwrap();
        let settings = without.get_connection_info().redis_settings();
        assert_eq!(settings.username(), None);
        assert_eq!(settings.password(), None);
    }

    #[test]
    fn split_command_follows_redis_cli_quoting() {
        let split = |line: &str| -> Vec<Vec<u8>> { split_command(line).unwrap() };
        let bytes = |args: &[&str]| -> Vec<Vec<u8>> {
            args.iter().map(|a| a.as_bytes().to_vec()).collect()
        };
        assert_eq!(split("  GET  key "), bytes(&["GET", "key"]));
        assert_eq!(
            split(r#"SET k "a b\n\x41\"""#),
            bytes(&["SET", "k", "a b\nA\""])
        );
        assert_eq!(split(r"SET k 'it\'s \n'"), bytes(&["SET", "k", r"it's \n"]));
        assert_eq!(split(r#"SET k """#), bytes(&["SET", "k", ""]));
        // \xHH is one byte, as in redis-cli; other text keeps its UTF-8.
        assert_eq!(
            split(r#"SET "\xe3\x81\x82" "\xff" あ"#),
            [
                b"SET".to_vec(),
                "あ".as_bytes().to_vec(),
                vec![0xff],
                "あ".as_bytes().to_vec()
            ]
        );
        assert!(split_command("").unwrap().is_empty());
        assert!(split_command(r#"GET "open"#).is_err());
        assert!(split_command(r#"GET "a"b"#).is_err());
        // Not two hex digits: the escape is the letter x, and the quote still closes.
        assert_eq!(split(r#"GET "a\x4""#), bytes(&["GET", "ax4"]));
        assert_eq!(split(r#"GET "\x+1""#), bytes(&["GET", "x+1"]));
    }

    #[test]
    fn dangerous_commands_are_recognized_however_they_are_written() {
        let check = |line: &str| needs_confirmation(&split_command(line).unwrap());
        assert_eq!(check("flushall"), Some("FLUSHALL".to_owned()));
        // Quoting the name must not get around the check.
        assert_eq!(check(r#""FLUSHALL" ASYNC"#), Some("FLUSHALL".to_owned()));
        assert_eq!(
            check("config set maxmemory 1"),
            Some("CONFIG SET".to_owned())
        );
        assert_eq!(check("CONFIG GET maxmemory"), None);
        assert_eq!(check("CONFIG"), None);
        assert_eq!(check("GET flushall"), None);
        assert_eq!(check("client pause 1000"), Some("CLIENT PAUSE".to_owned()));
    }

    #[test]
    fn commands_that_change_the_shared_connection_are_recognized() {
        let check = |line: &str| {
            find_command(
                &split_command(line).unwrap(),
                STATEFUL_COMMANDS,
                STATEFUL_SUBCOMMANDS,
            )
        };
        assert_eq!(check("multi"), Some("MULTI".to_owned()));
        assert_eq!(check("auth other pw"), Some("AUTH".to_owned()));
        assert_eq!(check("CLIENT REPLY OFF"), Some("CLIENT REPLY".to_owned()));
        assert_eq!(check("CLIENT LIST"), None);
        assert_eq!(check("GET multi"), None);
    }

    #[test]
    fn db_list_counts_keys_per_db() {
        let keyspace = "# Keyspace\r\ndb0:keys=309,expires=1,avg_ttl=0\r\ndb3:keys=2,expires=0\r\n";
        let dbs = db_list(Some(4), keyspace);
        assert_eq!(
            dbs.iter().map(|d| (d.db, d.keys)).collect::<Vec<_>>(),
            [(0, 309), (1, 0), (2, 0), (3, 2)]
        );
        // CONFIG GET denied: 16, or up to the highest DB listed.
        assert_eq!(db_list(None, "").len(), 16);
        assert_eq!(db_list(None, "db20:keys=1,expires=0").len(), 21);
    }

    #[test]
    fn command_docs_give_names_groups_and_syntax() {
        use redis::Value as V;
        let s = |t: &str| V::BulkString(t.as_bytes().to_vec());
        let arg = |fields: &[(&str, V)]| {
            V::Array(fields.iter().flat_map(|(k, v)| [s(k), v.clone()]).collect())
        };
        // SET key value [NX | XX] [EX seconds] and a CONFIG GET subcommand.
        let set = arg(&[
            ("summary", s("Sets the string value of a key.")),
            ("group", s("string")),
            (
                "arguments",
                V::Array(vec![
                    arg(&[
                        ("name", s("key")),
                        ("type", s("key")),
                        ("display_text", s("key")),
                    ]),
                    arg(&[("name", s("value")), ("type", s("string"))]),
                    arg(&[
                        ("name", s("condition")),
                        ("type", s("oneof")),
                        ("flags", V::Array(vec![V::SimpleString("optional".into())])),
                        (
                            "arguments",
                            V::Array(vec![
                                arg(&[
                                    ("name", s("nx")),
                                    ("type", s("pure-token")),
                                    ("token", s("NX")),
                                ]),
                                arg(&[
                                    ("name", s("xx")),
                                    ("type", s("pure-token")),
                                    ("token", s("XX")),
                                ]),
                            ]),
                        ),
                    ]),
                    arg(&[
                        ("name", s("seconds")),
                        ("type", s("integer")),
                        ("token", s("EX")),
                        ("flags", V::Array(vec![V::SimpleString("optional".into())])),
                    ]),
                    arg(&[
                        ("name", s("field")),
                        ("type", s("string")),
                        ("flags", V::Array(vec![V::SimpleString("multiple".into())])),
                    ]),
                ]),
            ),
        ]);
        let config = arg(&[
            ("summary", s("A container.")),
            ("group", s("server")),
            (
                "subcommands",
                V::Array(vec![s("config|get"), arg(&[("group", s("server"))])]),
            ),
        ]);
        let mut out = Vec::new();
        collect_docs(
            &V::Array(vec![s("set"), set, s("config"), config]),
            &mut out,
        );
        assert_eq!(out[0].name, "SET");
        assert_eq!(out[0].group, "string");
        assert_eq!(
            out[0].syntax,
            "key value [NX | XX] [EX seconds] field [field ...]"
        );
        assert_eq!(out[2].name, "CONFIG GET");
    }

    #[test]
    fn a_cut_character_is_dropped_from_a_truncated_string() {
        let mut cut = [b"ab".as_slice(), &"あ".as_bytes()[..2]].concat();
        trim_cut_character(&mut cut);
        assert_eq!(cut, b"ab");
        // A complete text, and bytes that are not UTF-8 at all, stay.
        let mut whole = "abあ".as_bytes().to_vec();
        trim_cut_character(&mut whole);
        assert_eq!(whole, "abあ".as_bytes());
        let mut binary = vec![b'a', 0xff, b'b'];
        trim_cut_character(&mut binary);
        assert_eq!(binary, [b'a', 0xff, b'b']);
    }

    #[test]
    fn keys_that_are_not_utf8_keep_their_bytes() {
        let plain = KeyEntry::new(b"user:1".to_vec(), "string".to_owned());
        assert_eq!((plain.key.as_str(), plain.raw.as_deref()), ("user:1", None));
        assert_eq!(key_bytes(&plain.key, None).unwrap(), b"user:1");

        let bytes = [b"sess:".as_slice(), &[0xff, 0x01], "あ".as_bytes()].concat();
        let binary = KeyEntry::new(bytes.clone(), "string".to_owned());
        // Shown as redis-cli writes it; the control byte 0x01 is valid UTF-8.
        assert_eq!(binary.key, "sess:\\xff\u{1}あ");
        assert_eq!(
            key_bytes(&binary.key, binary.raw.as_deref()).unwrap(),
            bytes
        );
        assert!(key_bytes("k", Some("f")).is_err());
        assert!(key_bytes("k", Some("zz")).is_err());
    }

    #[test]
    fn text_keeps_valid_utf8_and_replaces_the_rest() {
        assert_eq!(text("日本".as_bytes().to_vec()), "日本");
        assert_eq!(text(vec![b'a', 0xff]), "a\u{fffd}");
    }

    #[test]
    fn replies_are_formatted_like_redis_cli() {
        use redis::Value as V;
        assert_eq!(format_reply(&V::Nil), "(nil)");
        assert_eq!(format_reply(&V::Int(3)), "(integer) 3");
        assert_eq!(format_reply(&V::Okay), "OK");
        assert_eq!(format_reply(&V::BulkString(b"a\"b".to_vec())), r#""a\"b""#);
        assert_eq!(format_reply(&V::Array(vec![])), "(empty array)");
        assert_eq!(
            format_reply(&V::Array(vec![
                V::BulkString(b"x".to_vec()),
                V::Array(vec![V::Int(1), V::Int(2)]),
            ])),
            "1) \"x\"\n2) 1) (integer) 1\n   2) (integer) 2"
        );
        assert_eq!(
            format_reply(&V::Map(vec![(V::SimpleString("k".into()), V::Int(1))])),
            "1) k => (integer) 1"
        );
    }

    #[test]
    fn stream_entries_become_id_and_json_rows() {
        use redis::Value as V;
        let entries = V::Array(vec![V::Array(vec![
            V::BulkString(b"1-0".to_vec()),
            V::Array(vec![
                V::BulkString(b"user".to_vec()),
                V::BulkString(b"ann".to_vec()),
            ]),
        ])]);
        assert_eq!(stream_rows(&entries), [["1-0", r#"{"user":"ann"}"#]]);
    }
}
