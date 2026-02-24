#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mysql::{prelude::Queryable, OptsBuilder, Pool, Row, SslOpts, Value};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

#[derive(Debug, Deserialize, Serialize, Clone)]
struct MySqlConfig {
  host: String,
  port: u16,
  database: Option<String>,
  username: String,
  password: String,
  #[serde(default)]
  tls_enabled: bool,
  #[serde(default)]
  tls_skip_verify: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct SshConfig {
  enabled: bool,
  host: String,
  port: u16,
  username: String,
  private_key_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct ConnectionRequest {
  mysql: MySqlConfig,
  ssh: Option<SshConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ConnectionProfile {
  id: String,
  name: String,
  request: ConnectionRequest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct ConnectionProfileStore {
  version: u32,
  items: Vec<ConnectionProfile>,
}

#[derive(Debug, Serialize)]
struct QueryResult {
  columns: Vec<String>,
  rows: Vec<Vec<serde_json::Value>>,
  affected_rows: u64,
}

struct SshTunnel {
  child: Child,
  local_port: u16,
}

impl Drop for SshTunnel {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
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

  if mysql.tls_enabled {
    let mut ssl_opts = SslOpts::default();
    if mysql.tls_skip_verify {
      ssl_opts = ssl_opts
        .with_danger_accept_invalid_certs(true)
        .with_danger_skip_domain_validation(true);
    }
    builder = builder.ssl_opts(Some(ssl_opts));
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
      version: 1,
      items: Vec::new(),
    });
  }
  let content = std::fs::read_to_string(&path)
    .map_err(|e| format!("Failed to read profiles: {e}"))?;
  let store: ConnectionProfileStore =
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse profiles: {e}"))?;
  Ok(store)
}

fn save_profiles(app: &AppHandle, store: &ConnectionProfileStore) -> Result<(), String> {
  let path = profiles_path(app)?;
  if let Some(dir) = path.parent() {
    std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create config dir: {e}"))?;
  }
  let data =
    serde_json::to_string_pretty(store).map_err(|e| format!("Failed to serialize profiles: {e}"))?;
  std::fs::write(&path, data).map_err(|e| format!("Failed to write profiles: {e}"))?;
  Ok(())
}

fn generate_profile_id() -> String {
  use std::time::{SystemTime, UNIX_EPOCH};
  let ts = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_else(|_| Duration::from_millis(0))
    .as_millis();
  format!("p{ts}")
}

fn find_free_port() -> Result<u16, String> {
  let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("Failed to bind local port: {e}"))?;
  let addr = listener
    .local_addr()
    .map_err(|e| format!("Failed to read local addr: {e}"))?;
  Ok(addr.port())
}

fn wait_for_local_port(port: u16, timeout: Duration) -> Result<(), String> {
  let deadline = Instant::now() + timeout;
  let addr: SocketAddr = format!("127.0.0.1:{port}")
    .parse()
    .map_err(|e| format!("Invalid local address: {e}"))?;

  while Instant::now() < deadline {
    match TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
      Ok(_) => return Ok(()),
      Err(_) => thread::sleep(Duration::from_millis(120)),
    }
  }

  Err("SSH tunnel start timed out. Ensure ssh-agent is running and key is loaded.".to_string())
}

fn start_ssh_tunnel(ssh: &SshConfig, mysql_host: &str, mysql_port: u16) -> Result<SshTunnel, String> {
  let local_port = find_free_port()?;
  let ssh_bin = if cfg!(target_os = "windows") { "ssh.exe" } else { "ssh" };

  let mut args: Vec<String> = vec![
    "-N".to_string(),
    "-o".to_string(),
    "ExitOnForwardFailure=yes".to_string(),
    "-o".to_string(),
    "ServerAliveInterval=30".to_string(),
    "-o".to_string(),
    "ServerAliveCountMax=3".to_string(),
    "-p".to_string(),
    ssh.port.to_string(),
    "-L".to_string(),
    format!("127.0.0.1:{local_port}:{mysql_host}:{mysql_port}"),
  ];

  if let Some(key) = &ssh.private_key_path {
    if !key.trim().is_empty() {
      args.push("-i".to_string());
      args.push(key.clone());
    }
  }

  args.push(format!("{}@{}", ssh.username, ssh.host));

  let child = Command::new(ssh_bin)
    .args(args)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| {
      format!(
        "Failed to start {ssh_bin}. On Windows, install OpenSSH Client and run ssh-agent: {e}"
      )
    })?;

  let tunnel = SshTunnel { child, local_port };
  wait_for_local_port(local_port, Duration::from_secs(8))?;
  Ok(tunnel)
}

fn with_connection<R, F>(request: ConnectionRequest, f: F) -> Result<R, String>
where
  F: FnOnce(&mut mysql::PooledConn) -> Result<R, String>,
{
  let (target_host, target_port, _tunnel): (String, u16, Option<SshTunnel>) = match request.ssh {
    Some(ssh) if ssh.enabled => {
      let tunnel = start_ssh_tunnel(&ssh, &request.mysql.host, request.mysql.port)?;
      ("127.0.0.1".to_string(), tunnel.local_port, Some(tunnel))
    }
    _ => (request.mysql.host.clone(), request.mysql.port, None),
  };

  let opts = build_opts(&request.mysql, &target_host, target_port);
  let pool = Pool::new(opts).map_err(|e| format!("Failed to build pool: {e}"))?;
  let mut conn = pool
    .get_conn()
    .map_err(|e| format!("Failed to connect MySQL: {e}"))?;

  let result = f(&mut conn)?;
  drop(conn);
  drop(pool);
  Ok(result)
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
      json!(format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}.{:06}", micros))
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

#[tauri::command]
fn test_connection(request: ConnectionRequest) -> Result<String, String> {
  with_connection(request, |conn| {
    conn.query_drop("SELECT 1")
      .map_err(|e| format!("MySQL ping failed: {e}"))?;
    Ok("Connection succeeded.".to_string())
  })
}

#[tauri::command]
fn run_query(request: ConnectionRequest, query: String) -> Result<QueryResult, String> {
  if query.trim().is_empty() {
    return Err("Query is empty".to_string());
  }

  with_connection(request, |conn| {
    let mut result = conn
      .query_iter(query)
      .map_err(|e| format!("Query failed: {e}"))?;

    let columns = result
      .columns()
      .as_ref()
      .iter()
      .map(|c| c.name_str().to_string())
      .collect::<Vec<_>>();

    let mut rows: Vec<Vec<serde_json::Value>> = Vec::new();
    while let Some(next_row) = result.next() {
      let row = next_row.map_err(|e| format!("Row read failed: {e}"))?;
      rows.push(row_to_json(row));
      if rows.len() >= 500 {
        break;
      }
    }

    Ok(QueryResult {
      columns,
      rows,
      affected_rows: result.affected_rows(),
    })
  })
}

#[tauri::command]
fn list_profiles(app: AppHandle) -> Result<Vec<ConnectionProfile>, String> {
  let store = load_profiles(&app)?;
  Ok(store.items)
}

#[tauri::command]
fn save_profile(app: AppHandle, mut profile: ConnectionProfile) -> Result<Vec<ConnectionProfile>, String> {
  if profile.name.trim().is_empty() {
    return Err("Profile name is empty".to_string());
  }
  if profile.id.trim().is_empty() {
    profile.id = generate_profile_id();
  }
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
  if store.version == 0 {
    store.version = 1;
  }
  save_profiles(&app, &store)?;
  Ok(store.items)
}

#[tauri::command]
fn delete_profile(app: AppHandle, id: String) -> Result<Vec<ConnectionProfile>, String> {
  let mut store = load_profiles(&app)?;
  store.items.retain(|item| item.id != id);
  save_profiles(&app, &store)?;
  Ok(store.items)
}

fn main() {
  tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![
      test_connection,
      run_query,
      list_profiles,
      save_profile,
      delete_profile
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
