#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use mysql::{prelude::Queryable, OptsBuilder, Pool, Row, SslOpts, Value};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, Window};

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
  request: ConnectionRequest,
}

#[derive(Debug, Serialize, Deserialize, Default)]
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

struct ConnectionCache {
  fingerprint: String,
  pool: Pool,
  _tunnel: Option<SshTunnel>,
}

fn connection_fingerprint(request: &ConnectionRequest) -> String {
  let ssh_part = match &request.ssh {
    Some(ssh) if ssh.enabled => format!(
      "ssh:{}:{}:{}:{}",
      ssh.host, ssh.port, ssh.username,
      ssh.private_key_path.as_deref().unwrap_or("")
    ),
    _ => String::new(),
  };
  format!(
    "{}:{}:{}:{}:tls{}:skip{}|{}",
    request.mysql.host, request.mysql.port,
    request.mysql.username, request.mysql.password,
    request.mysql.tls_enabled, request.mysql.tls_skip_verify,
    ssh_part
  )
}

fn get_or_create_pool(
  cache: &Mutex<Option<ConnectionCache>>,
  request: &ConnectionRequest,
) -> Result<Pool, String> {
  let fp = connection_fingerprint(request);
  let mut guard = cache.lock().map_err(|e| format!("Lock error: {e}"))?;

  if let Some(ref cached) = *guard {
    if cached.fingerprint == fp {
      return Ok(cached.pool.clone());
    }
  }

  // Drop old cache first (frees SSH tunnel port)
  *guard = None;

  let (target_host, target_port, tunnel) = match &request.ssh {
    Some(ssh) if ssh.enabled => {
      let tunnel = start_ssh_tunnel(ssh, &request.mysql.host, request.mysql.port)?;
      ("127.0.0.1".to_string(), tunnel.local_port, Some(tunnel))
    }
    _ => (request.mysql.host.clone(), request.mysql.port, None),
  };

  // Build opts WITHOUT database — we USE db per-query
  let mut mysql_no_db = request.mysql.clone();
  mysql_no_db.database = None;
  let opts = build_opts(&mysql_no_db, &target_host, target_port);
  let pool = Pool::new(opts).map_err(|e| format!("Failed to build pool: {e}"))?;

  // Verify the connection works
  let _conn = pool.get_conn().map_err(|e| format!("Failed to connect MySQL: {e}"))?;

  *guard = Some(ConnectionCache {
    fingerprint: fp,
    pool: pool.clone(),
    _tunnel: tunnel,
  });

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
      version: 2,
      groups: Vec::new(),
      items: Vec::new(),
    });
  }
  let content = std::fs::read_to_string(&path)
    .map_err(|e| format!("Failed to read profiles: {e}"))?;
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


fn find_ssh_bin() -> String {
  if cfg!(target_os = "windows") {
    // Prefer Windows built-in OpenSSH over Git-bundled ssh.exe
    // because 1Password SSH agent only works with the Windows version.
    let system_ssh = PathBuf::from(r"C:\Windows\System32\OpenSSH\ssh.exe");
    if system_ssh.exists() {
      return system_ssh.to_string_lossy().into_owned();
    }
    "ssh.exe".to_string()
  } else {
    "ssh".to_string()
  }
}

fn start_ssh_tunnel(ssh: &SshConfig, mysql_host: &str, mysql_port: u16) -> Result<SshTunnel, String> {
  let local_port = find_free_port()?;
  let ssh_bin = find_ssh_bin();

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
      args.push("-o".to_string());
      args.push("IdentitiesOnly=yes".to_string());
      args.push("-o".to_string());
      args.push(format!("IdentityFile={key}"));
    }
  }

  args.push(format!("{}@{}", ssh.username, ssh.host));

  let mut child = Command::new(&ssh_bin)
    .args(&args)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|e| {
      format!(
        "Failed to start {ssh_bin}. On Windows, install OpenSSH Client and run ssh-agent: {e}"
      )
    })?;

  let deadline = Instant::now() + Duration::from_secs(8);
  let addr: SocketAddr = format!("127.0.0.1:{local_port}")
    .parse()
    .map_err(|e| format!("Invalid local address: {e}"))?;

  while Instant::now() < deadline {
    if let Ok(Some(status)) = child.try_wait() {
      let stderr_msg = read_child_stderr(&mut child);
      return Err(format!(
        "SSH process exited with {status}. cmd: {ssh_bin} {}\nstderr: {stderr_msg}",
        args.join(" ")
      ));
    }
    match TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
      Ok(_) => return Ok(SshTunnel { child, local_port }),
      Err(_) => thread::sleep(Duration::from_millis(120)),
    }
  }

  let stderr_msg = read_child_stderr(&mut child);
  let _ = child.kill();
  let _ = child.wait();
  Err(format!(
    "SSH tunnel timed out. cmd: {ssh_bin} {}\nstderr: {stderr_msg}",
    args.join(" ")
  ))
}

fn read_child_stderr(child: &mut Child) -> String {
  use std::io::Read;
  child
    .stderr
    .take()
    .and_then(|mut s| {
      let mut buf = String::new();
      s.read_to_string(&mut buf).ok().map(|_| buf)
    })
    .unwrap_or_default()
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
fn run_query(
  request: ConnectionRequest,
  query: String,
  max_rows: Option<usize>,
  state: tauri::State<'_, Mutex<Option<ConnectionCache>>>,
) -> Result<QueryResult, String> {
  if query.trim().is_empty() {
    return Err("Query is empty".to_string());
  }

  let limit = max_rows.unwrap_or(500);

  let pool = get_or_create_pool(&state, &request)?;
  let mut conn = pool.get_conn().map_err(|e| format!("Failed to get connection: {e}"))?;

  // Switch database if specified
  if let Some(ref db) = request.mysql.database {
    if !db.trim().is_empty() {
      conn.query_drop(format!("USE `{}`", db))
        .map_err(|e| format!("Failed to switch database: {e}"))?;
    }
  }

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
    if limit > 0 && rows.len() >= limit {
      break;
    }
  }

  Ok(QueryResult {
    columns,
    rows,
    affected_rows: result.affected_rows(),
  })
}

#[tauri::command]
fn disconnect_pool(
  state: tauri::State<'_, Mutex<Option<ConnectionCache>>>,
) -> Result<(), String> {
  let mut guard = state.lock().map_err(|e| format!("Lock error: {e}"))?;
  *guard = None;
  Ok(())
}

#[tauri::command]
async fn export_file(
  content: String,
  default_name: String,
  filter_name: String,
  extensions: Vec<String>,
) -> Result<bool, String> {
  let ext_refs: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
  let dialog = rfd::AsyncFileDialog::new()
    .set_file_name(&default_name)
    .add_filter(&filter_name, &ext_refs)
    .save_file()
    .await;

  match dialog {
    Some(handle) => {
      std::fs::write(handle.path(), content.as_bytes())
        .map_err(|e| format!("Failed to write file: {e}"))?;
      Ok(true)
    }
    None => Ok(false),
  }
}

#[tauri::command]
fn list_profiles(app: AppHandle) -> Result<ProfileListResponse, String> {
  let store = load_profiles(&app)?;
  Ok(ProfileListResponse {
    groups: store.groups,
    items: store.items,
  })
}

#[tauri::command]
fn save_profile(app: AppHandle, mut profile: ConnectionProfile) -> Result<ProfileListResponse, String> {
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
  })
}

#[tauri::command]
fn delete_profile(app: AppHandle, id: String) -> Result<ProfileListResponse, String> {
  let mut store = load_profiles(&app)?;
  store.items.retain(|item| item.id != id);
  save_profiles(&app, &store)?;
  Ok(ProfileListResponse {
    groups: store.groups,
    items: store.items,
  })
}

#[tauri::command]
fn save_group(app: AppHandle, id: Option<String>, name: String) -> Result<ProfileListResponse, String> {
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
  let new_profile = ConnectionProfile {
    id: generate_profile_id(),
    name: format!("{} (copy)", source.name),
    group_id: source.group_id,
    order: max_order + 1000,
    request: source.request,
  };
  store.items.push(new_profile);
  save_profiles(&app, &store)?;
  Ok(ProfileListResponse {
    groups: store.groups,
    items: store.items,
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
  })
}

#[tauri::command]
fn open_settings_window(app: AppHandle, id: Option<String>) -> Result<(), String> {
  let window = app
    .get_webview_window("settings")
    .ok_or("Settings window not found")?;
  window
    .emit("settings:open", id)
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
    .get_webview_window("query")
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
  Ok(())
}

#[tauri::command]
fn hide_window(window: Window) -> Result<(), String> {
  window
    .hide()
    .map_err(|e| format!("Failed to hide window: {e}"))
}

fn main() {
  tauri::Builder::default()
    .manage(Mutex::new(None::<ConnectionCache>))
    .on_window_event(|window, event| {
      if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() != "main" {
          api.prevent_close();
          let _ = window.hide();
        }
      }
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
      export_file
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
