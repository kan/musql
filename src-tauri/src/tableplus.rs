//! Reads the connections TablePlus keeps (#99) and turns them into profiles, which
//! `import_tableplus` merges as an import file would be.
//!
//! TablePlus for Windows keeps them in `%LOCALAPPDATA%\com.tinyapp.TablePlus\Data`
//! (or the folder `sharedConnectionPath` in `Setting.json` names): `Connections.plist`
//! is an array of one dictionary per connection, `ConnectionGroups.plist` the groups.
//! The keys are those of the macOS version.
//!
//! Passwords are not read. TablePlus for Windows encrypts them into its own file
//! (`.tpmaster`) with a key it keeps in the Credential Manager, a format of its own. The
//! imported profiles ask for them on connecting (`save_password` is false), unless
//! TablePlus says the connection has none.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use plist::{Dictionary, Value};

use crate::{
    ssh_dir, ConnectionProfile, ConnectionRequest, Engine, ExportData, MySqlConfig, ProfileGroup,
    SshConfig, SslMode,
};

const CONNECTIONS_FILE: &str = "Connections.plist";
const GROUPS_FILE: &str = "ConnectionGroups.plist";

/// `DatabasePasswordMode` / `ServerPasswordMode`: 0 keeps the password, 1 asks every
/// time, 2 is "no password".
const NO_PASSWORD: i64 = 2;

/// The profiles read, and the connections left out (as "name (driver)"): those of a
/// kind muSQL does not connect to.
pub struct Imported {
    pub data: ExportData,
    pub skipped: Vec<String>,
}

/// The folder that holds `Connections.plist`.
pub fn data_dir() -> Result<PathBuf, String> {
    let local = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is not set.")?;
    let base = Path::new(&local).join("com.tinyapp.TablePlus");
    Ok(shared_connection_path(&base.join("Setting.json")).unwrap_or_else(|| base.join("Data")))
}

/// The folder TablePlus was told to keep the connections in, if any.
fn shared_connection_path(settings: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(settings).ok()?;
    let json: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let path = json.get("sharedConnectionPath")?.as_str()?.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Reads the connections in `dir`. The groups file is optional; the connections file is not.
pub fn read(dir: &Path) -> Result<Imported, String> {
    let path = dir.join(CONNECTIONS_FILE);
    if !path.is_file() {
        return Err(format!(
            "TablePlus connections were not found ({}).",
            path.display()
        ));
    }
    let connections =
        Value::from_file(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let groups = Value::from_file(dir.join(GROUPS_FILE)).ok();
    convert(&connections, groups.as_ref(), ssh_dir().as_deref(), |p| {
        p.is_file()
    })
}

/// Turns the two plists into profiles. `ssh_dir` is where a bare key file name is looked
/// for (`~/.ssh`), and `key_exists` checks a key file (a parameter for the tests).
fn convert(
    connections: &Value,
    groups: Option<&Value>,
    ssh_dir: Option<&Path>,
    key_exists: impl Fn(&Path) -> bool,
) -> Result<Imported, String> {
    let entries = connections
        .as_array()
        .ok_or("Connections.plist is not a list of connections.")?;
    let group_names = group_paths(groups);

    let mut data = ExportData {
        version: 1,
        groups: Vec::new(),
        items: Vec::new(),
        passwords: None,
        ssh_passphrases: None,
    };
    let mut skipped = Vec::new();
    for entry in entries.iter().filter_map(Value::as_dictionary) {
        let name = text(entry, "ConnectionName");
        let name = if name.is_empty() {
            "(unnamed)".to_owned()
        } else {
            name
        };
        let driver = text(entry, "Driver");
        let Some(request) = request(entry, &driver, ssh_dir, &key_exists) else {
            skipped.push(format!("{name} ({driver})"));
            continue;
        };
        // Groups of the same name (or path) become one, as muSQL tells groups apart by
        // name when it matches an import against the profiles it has.
        let group_id = group_names.get(&text(entry, "GroupID")).map(|name| {
            match data.groups.iter().find(|g| g.name == *name) {
                Some(group) => group.id.clone(),
                None => {
                    let id = text(entry, "GroupID");
                    data.groups.push(ProfileGroup {
                        id: id.clone(),
                        name: name.clone(),
                        order: 0,
                    });
                    id
                }
            }
        });
        data.items.push(ConnectionProfile {
            // The import gives every profile a new ID (`merge_import`).
            id: text(entry, "ID"),
            name,
            group_id,
            order: 0,
            color: color(&text(entry, "statusColor")),
            tags: environment_tag(&text(entry, "Enviroment"))
                .into_iter()
                .collect(),
            request,
            clear_password: false,
            clear_ssh_passphrase: false,
            clear_ssh_password: false,
        });
    }
    Ok(Imported { data, skipped })
}

/// The group of each group ID, named by its path: muSQL's groups do not nest, so a group
/// inside another is named "Parent / Child".
fn group_paths(groups: Option<&Value>) -> HashMap<String, String> {
    let mut parent_and_name = HashMap::new();
    for group in groups
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_dictionary)
    {
        let id = text(group, "ID");
        if !id.is_empty() {
            parent_and_name.insert(id, (text(group, "GroupID"), text(group, "Name")));
        }
    }
    parent_and_name
        .keys()
        .map(|id| {
            let mut names = Vec::new();
            let mut current = id.as_str();
            // Bounded, so a parent chain that loops cannot hang the import.
            while let Some((parent, name)) = parent_and_name.get(current) {
                names.push(name.as_str());
                if names.len() >= 16 {
                    break;
                }
                current = parent;
            }
            names.reverse();
            (id.clone(), names.join(" / "))
        })
        .collect()
}

/// The connection, or `None` for a kind muSQL does not connect to.
fn request(
    entry: &Dictionary,
    driver: &str,
    ssh_dir: Option<&Path>,
    key_exists: &impl Fn(&Path) -> bool,
) -> Option<ConnectionRequest> {
    let engine = match driver {
        "MySQL" => Engine::Mysql,
        "MariaDB" => Engine::Mariadb,
        "PostgreSQL" => Engine::Postgresql,
        "Redis" => Engine::Redis,
        "SQLite" => Engine::Sqlite,
        _ => return None,
    };
    let mut mysql = MySqlConfig {
        host: String::new(),
        port: 0,
        database: non_empty(text(entry, "DatabaseName")),
        username: String::new(),
        password: String::new(),
        ssl_mode: SslMode::Disabled,
        tls_ca_cert_path: None,
        save_password: number(entry, "DatabasePasswordMode") == Some(NO_PASSWORD),
        op_ref: None,
        file_path: None,
        tls_enabled: false,
        tls_skip_verify: false,
    };
    // A file: no server, TLS or SSH tunnel.
    let Some(default_port) = engine.default_port() else {
        mysql.file_path = Some(non_empty(text(entry, "DatabasePath"))?);
        return Some(ConnectionRequest {
            engine,
            mysql,
            ssh: None,
        });
    };
    mysql.host = text(entry, "DatabaseHost");
    mysql.port = port(entry, "DatabasePort").unwrap_or(default_port);
    mysql.username = text(entry, "DatabaseUser");
    mysql.ssl_mode = ssl_mode(driver, number(entry, "tLSMode"));
    // The slots are client key, client certificate, CA certificate.
    mysql.tls_ca_cert_path = entry
        .get("TlsKeyPaths")
        .and_then(Value::as_array)
        .and_then(|paths| paths.get(2))
        .and_then(Value::as_string)
        .and_then(|p| non_empty(p.trim().to_owned()));
    let ssh = flag(entry, "isOverSSH").then(|| ssh(entry, ssh_dir, key_exists));
    Some(ConnectionRequest { engine, mysql, ssh })
}

fn ssh(
    entry: &Dictionary,
    ssh_dir: Option<&Path>,
    key_exists: &impl Fn(&Path) -> bool,
) -> SshConfig {
    let use_key = flag(entry, "isUsePrivateKey");
    SshConfig {
        enabled: true,
        host: text(entry, "ServerAddress"),
        port: port(entry, "ServerPort").unwrap_or(22),
        username: text(entry, "ServerUser"),
        private_key_path: if use_key {
            key_path(&text(entry, "ServerPrivateKeyName"), ssh_dir, key_exists)
        } else {
            None
        },
        config_host: None,
        passphrase: String::new(),
        auth_method: if use_key { "key" } else { "password" }.to_owned(),
        ssh_password: String::new(),
        save_ssh_password: number(entry, "ServerPasswordMode") == Some(NO_PASSWORD),
        // Unknown whether the key has one: asking for it on every connection would be
        // in the way of the many keys that have none. A key that needs one fails to
        // load, and the profile is edited then.
        save_ssh_passphrase: true,
        op_passphrase_ref: None,
        op_password_ref: None,
    }
}

/// The key file. TablePlus for Windows keeps a bare file name, which is looked for in
/// `~/.ssh`. A key that is not there (or, for a full path, no longer there) is left out,
/// so muSQL falls back to the agent and the default keys instead of failing on a path
/// that does not exist.
fn key_path(
    name: &str,
    ssh_dir: Option<&Path>,
    key_exists: &impl Fn(&Path) -> bool,
) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    let candidate = if Path::new(name).is_absolute() {
        PathBuf::from(name)
    } else if let Some(rest) = name.strip_prefix("~/").or_else(|| name.strip_prefix("~\\")) {
        // Under the home folder, which holds `~/.ssh`.
        ssh_dir?
            .parent()?
            .join(rest.split(['/', '\\']).collect::<PathBuf>())
    } else {
        ssh_dir?.join(name)
    };
    key_exists(&candidate).then(|| candidate.to_string_lossy().into_owned())
}

/// TablePlus's TLS mode is an index into the choices of its form, which differ by
/// driver (the order follows TablePro's importer, which reads the same files). Without
/// one, the form's first choice (TablePlus's default) applies. PostgreSQL's "allow"
/// (plain text unless the server insists on TLS), which muSQL does not have, becomes
/// the nearest, `PREFERRED`. An index past the choices becomes `REQUIRED`, so that a mode
/// this list does not know does not silently drop TLS.
fn ssl_mode(driver: &str, index: Option<i64>) -> SslMode {
    use SslMode::*;
    let modes: &[SslMode] = match driver {
        "MySQL" => &[Preferred, Disabled, Required, VerifyCa, VerifyIdentity],
        "MariaDB" => &[Preferred, Required, VerifyIdentity],
        "PostgreSQL" => &[
            Preferred,
            Disabled,
            Required,
            Preferred,
            VerifyCa,
            VerifyIdentity,
        ],
        "Redis" => &[Disabled, Required, VerifyCa],
        _ => &[Disabled],
    };
    usize::try_from(index.unwrap_or(0))
        .ok()
        .and_then(|i| modes.get(i))
        .cloned()
        .unwrap_or(Required)
}

/// TablePlus's own colour (`#RRGGBB`), if one was picked.
fn color(value: &str) -> Option<String> {
    let hex = value.strip_prefix('#')?;
    (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{}", hex.to_ascii_lowercase()))
}

/// TablePlus's environment label as a tag, spelled as muSQL's preset tags are.
fn environment_tag(value: &str) -> Option<String> {
    match value {
        "" => None,
        "testing" => Some("test".to_owned()),
        other => Some(other.to_owned()),
    }
}

fn non_empty(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

/// A port, `None` when unset or 0 (the caller's default then applies).
fn port(dict: &Dictionary, key: &str) -> Option<u16> {
    number(dict, key)
        .and_then(|p| u16::try_from(p).ok())
        .filter(|&p| p != 0)
}

/// A switch TablePlus writes as 0 / 1 (or a boolean); absent is off.
fn flag(dict: &Dictionary, key: &str) -> bool {
    number(dict, key).is_some_and(|n| n != 0)
}

fn text(dict: &Dictionary, key: &str) -> String {
    match dict.get(key) {
        Some(Value::String(s)) => s.trim().to_owned(),
        Some(Value::Integer(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// A number TablePlus writes as an integer, a boolean or a string (the ports).
fn number(dict: &Dictionary, key: &str) -> Option<i64> {
    match dict.get(key)? {
        Value::Integer(n) => n.as_signed(),
        Value::Boolean(b) => Some(i64::from(*b)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plist(xml: &str) -> Value {
        let doc = format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">{xml}</plist>"#
        );
        Value::from_reader_xml(doc.as_bytes()).unwrap()
    }

    // The shape TablePlus for Windows writes (the ports as strings, flags as integers).
    const MYSQL_OVER_SSH: &str = r#"<dict>
        <key>ID</key><string>11111111-1111-1111-1111-111111111111</string>
        <key>GroupID</key><string>G2</string>
        <key>ConnectionName</key><string>prod db</string>
        <key>Enviroment</key><string>production</string>
        <key>Driver</key><string>MySQL</string>
        <key>DatabaseHost</key><string>db.internal</string>
        <key>DatabasePort</key><string>3307</string>
        <key>DatabaseUser</key><string>app</string>
        <key>DatabaseName</key><string>shop</string>
        <key>tLSMode</key><integer>3</integer>
        <key>TlsKeyPaths</key><array><string></string><string></string><string>C:\ca.pem</string></array>
        <key>isOverSSH</key><integer>1</integer>
        <key>ServerAddress</key><string>bastion.example.com</string>
        <key>ServerPort</key><string>10022</string>
        <key>ServerUser</key><string>ops</string>
        <key>isUsePrivateKey</key><integer>1</integer>
        <key>ServerPrivateKeyName</key><string>id_ed25519</string>
        <key>statusColor</key><string>#6D0000</string>
        <key>DatabasePasswordMode</key><integer>0</integer>
        <key>ServerPasswordMode</key><integer>0</integer>
    </dict>"#;

    const GROUPS: &str = r#"<array>
        <dict><key>ID</key><string>G1</string><key>GroupID</key><string></string><key>Name</key><string>Work</string></dict>
        <dict><key>ID</key><string>G2</string><key>GroupID</key><string>G1</string><key>Name</key><string>Shop</string></dict>
    </array>"#;

    fn import(connections: &str, groups: Option<&str>) -> Imported {
        let groups = groups.map(plist);
        convert(
            &plist(connections),
            groups.as_ref(),
            Some(Path::new(r"C:\Users\me\.ssh")),
            |p| p.ends_with("id_ed25519"),
        )
        .unwrap()
    }

    #[test]
    fn reads_a_mysql_connection_over_ssh() {
        let imported = import(&format!("<array>{MYSQL_OVER_SSH}</array>"), Some(GROUPS));
        assert!(imported.skipped.is_empty());
        let [item] = imported.data.items.as_slice() else {
            panic!("one item")
        };
        assert_eq!(item.name, "prod db");
        assert_eq!(item.color.as_deref(), Some("#6d0000"));
        assert_eq!(item.tags, ["production"]);
        let req = &item.request;
        assert_eq!(req.engine, Engine::Mysql);
        assert_eq!(
            (
                req.mysql.host.as_str(),
                req.mysql.port,
                req.mysql.username.as_str()
            ),
            ("db.internal", 3307, "app")
        );
        assert_eq!(req.mysql.database.as_deref(), Some("shop"));
        assert_eq!(req.mysql.ssl_mode, SslMode::VerifyCa);
        assert_eq!(req.mysql.tls_ca_cert_path.as_deref(), Some(r"C:\ca.pem"));
        // The password stays in TablePlus: asked for on connecting.
        assert!(!req.mysql.save_password);
        let ssh = req.ssh.as_ref().unwrap();
        assert_eq!(
            (
                ssh.host.as_str(),
                ssh.port,
                ssh.username.as_str(),
                ssh.auth_method.as_str()
            ),
            ("bastion.example.com", 10022, "ops", "key")
        );
        assert_eq!(
            ssh.private_key_path.as_deref(),
            Some(r"C:\Users\me\.ssh\id_ed25519")
        );
    }

    #[test]
    fn nested_groups_are_named_by_their_path() {
        let imported = import(&format!("<array>{MYSQL_OVER_SSH}</array>"), Some(GROUPS));
        let [group] = imported.data.groups.as_slice() else {
            panic!("one group: only the groups used are imported")
        };
        assert_eq!(group.name, "Work / Shop");
        assert_eq!(
            imported.data.items[0].group_id.as_deref(),
            Some(group.id.as_str())
        );
    }

    #[test]
    fn a_group_that_is_not_listed_is_dropped() {
        let imported = import(&format!("<array>{MYSQL_OVER_SSH}</array>"), None);
        assert!(imported.data.groups.is_empty());
        assert_eq!(imported.data.items[0].group_id, None);
    }

    #[test]
    fn a_key_not_in_the_ssh_folder_is_left_out() {
        let conn = MYSQL_OVER_SSH.replace("id_ed25519", "id_missing");
        let imported = import(&format!("<array>{conn}</array>"), None);
        let ssh = imported.data.items[0].request.ssh.clone().unwrap();
        assert_eq!(ssh.private_key_path, None);
        assert_eq!(ssh.auth_method, "key");
    }

    #[test]
    fn key_paths_resolve_from_home_and_must_exist() {
        let ssh = Some(Path::new(r"C:\Users\me\.ssh"));
        let exists = |p: &Path| p == Path::new(r"C:\Users\me\keys\prod.pem");
        assert_eq!(
            key_path("~/keys/prod.pem", ssh, &exists).as_deref(),
            Some(r"C:\Users\me\keys\prod.pem")
        );
        // A full path to a key that has gone is left out, as a bare name is.
        assert_eq!(key_path(r"C:\old\id_rsa", ssh, &exists), None);
    }

    #[test]
    fn groups_of_the_same_name_become_one() {
        let conn_b = MYSQL_OVER_SSH
            .replace("<string>G2</string>", "<string>G3</string>")
            .replace("prod db", "other");
        let groups = r#"<array>
            <dict><key>ID</key><string>G2</string><key>GroupID</key><string></string><key>Name</key><string>Shop</string></dict>
            <dict><key>ID</key><string>G3</string><key>GroupID</key><string></string><key>Name</key><string>Shop</string></dict>
        </array>"#;
        let imported = import(
            &format!("<array>{MYSQL_OVER_SSH}{conn_b}</array>"),
            Some(groups),
        );
        assert_eq!(imported.data.groups.len(), 1);
        let ids: Vec<_> = imported
            .data
            .items
            .iter()
            .map(|i| i.group_id.clone())
            .collect();
        assert_eq!(ids, [Some("G2".to_owned()), Some("G2".to_owned())]);
    }

    #[test]
    fn a_port_of_zero_takes_the_default() {
        let conn = MYSQL_OVER_SSH
            .replace("<string>3307</string>", "<string>0</string>")
            .replace("<string>10022</string>", "<string>0</string>");
        let imported = import(&format!("<array>{conn}</array>"), None);
        let req = &imported.data.items[0].request;
        assert_eq!(req.mysql.port, 3306);
        assert_eq!(req.ssh.as_ref().unwrap().port, 22);
    }

    #[test]
    fn other_kinds_are_skipped_by_name() {
        let imported = import(
            r#"<array>
                <dict><key>ConnectionName</key><string>mongo</string><key>Driver</key><string>Mongo</string></dict>
                <dict><key>ConnectionName</key><string>file</string><key>Driver</key><string>SQLite</string></dict>
            </array>"#,
            None,
        );
        assert!(imported.data.items.is_empty());
        // A SQLite connection without its file has nothing to open.
        assert_eq!(imported.skipped, ["mongo (Mongo)", "file (SQLite)"]);
    }

    #[test]
    fn missing_values_fall_back_to_the_engine_defaults() {
        let imported = import(
            r#"<array><dict>
                <key>ConnectionName</key><string>cache</string>
                <key>Driver</key><string>Redis</string>
                <key>DatabaseHost</key><string>127.0.0.1</string>
                <key>DatabasePort</key><string></string>
                <key>DatabasePasswordMode</key><integer>2</integer>
                <key>Enviroment</key><string>testing</string>
            </dict></array>"#,
            None,
        );
        let item = &imported.data.items[0];
        assert_eq!(item.request.engine, Engine::Redis);
        assert_eq!(item.request.mysql.port, 6379);
        assert_eq!(item.request.mysql.database, None);
        assert_eq!(item.request.mysql.ssl_mode, SslMode::Disabled);
        assert!(item.request.ssh.is_none());
        // "No password": nothing to ask for.
        assert!(item.request.mysql.save_password);
        assert_eq!(item.tags, ["test"]);
        assert_eq!(item.color, None);
    }

    #[test]
    fn sqlite_keeps_only_the_file() {
        let imported = import(
            r#"<array><dict>
                <key>ConnectionName</key><string>local</string>
                <key>Driver</key><string>SQLite</string>
                <key>DatabasePath</key><string>C:\data\app.db</string>
                <key>DatabaseHost</key><string>ignored</string>
                <key>isOverSSH</key><integer>1</integer>
            </dict></array>"#,
            None,
        );
        let req = &imported.data.items[0].request;
        assert_eq!(req.engine, Engine::Sqlite);
        assert_eq!(req.mysql.file_path.as_deref(), Some(r"C:\data\app.db"));
        assert_eq!((req.mysql.host.as_str(), req.mysql.port), ("", 0));
        assert!(req.ssh.is_none());
    }

    #[test]
    fn tls_modes_follow_the_form_of_each_driver() {
        assert_eq!(ssl_mode("MySQL", Some(0)), SslMode::Preferred);
        assert_eq!(ssl_mode("MySQL", Some(1)), SslMode::Disabled);
        assert_eq!(ssl_mode("MySQL", Some(2)), SslMode::Required);
        assert_eq!(ssl_mode("MySQL", Some(4)), SslMode::VerifyIdentity);
        assert_eq!(ssl_mode("MariaDB", Some(2)), SslMode::VerifyIdentity);
        assert_eq!(ssl_mode("PostgreSQL", Some(3)), SslMode::Preferred); // allow
        assert_eq!(ssl_mode("PostgreSQL", Some(4)), SslMode::VerifyCa);
        assert_eq!(ssl_mode("Redis", Some(1)), SslMode::Required);
        // Unset: the form's default.
        assert_eq!(ssl_mode("MySQL", None), SslMode::Preferred);
        assert_eq!(ssl_mode("Redis", None), SslMode::Disabled);
        // Past the known choices: TLS is kept on rather than dropped.
        assert_eq!(ssl_mode("MySQL", Some(9)), SslMode::Required);
    }

    #[test]
    fn a_looping_group_chain_ends() {
        let groups = plist(
            r#"<array>
                <dict><key>ID</key><string>A</string><key>GroupID</key><string>B</string><key>Name</key><string>a</string></dict>
                <dict><key>ID</key><string>B</string><key>GroupID</key><string>A</string><key>Name</key><string>b</string></dict>
            </array>"#,
        );
        let paths = group_paths(Some(&groups));
        assert_eq!(paths.len(), 2);
    }

    /// Reads the TablePlus of this machine, if any (`cargo test tableplus -- --ignored
    /// --nocapture`). Prints counts and kinds only: hosts and names stay out of the output.
    #[test]
    #[ignore]
    fn reads_the_tableplus_of_this_machine() {
        let dir = data_dir().unwrap();
        if !dir.join(CONNECTIONS_FILE).is_file() {
            println!(
                "tableplus: skipped (no {})",
                dir.join(CONNECTIONS_FILE).display()
            );
            return;
        }
        let imported = read(&dir).unwrap();
        let mut kinds: HashMap<String, usize> = HashMap::new();
        for item in &imported.data.items {
            let r = &item.request;
            let ssh = r.ssh.as_ref().map_or("direct".to_owned(), |s| {
                let key = if s.private_key_path.is_some() {
                    "key file"
                } else {
                    "no key file"
                };
                format!("ssh {} {key}", s.auth_method)
            });
            let ssl: String = r.mysql.ssl_mode.clone().into();
            *kinds
                .entry(format!("{} / {ssl} / {ssh}", r.engine.display_name()))
                .or_default() += 1;
        }
        println!(
            "tableplus: {} profiles, {} groups, {} skipped",
            imported.data.items.len(),
            imported.data.groups.len(),
            imported.skipped.len()
        );
        for (kind, n) in kinds {
            println!("  {n:3}  {kind}");
        }
    }

    #[test]
    fn shared_connection_path_is_read_from_the_settings() {
        let dir = std::env::temp_dir().join(format!("musql-tableplus-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("Setting.json");
        std::fs::write(
            &settings,
            "\u{feff}{\"sharedConnectionPath\": \"D:\\\\tp\"}",
        )
        .unwrap();
        assert_eq!(
            shared_connection_path(&settings),
            Some(PathBuf::from(r"D:\tp"))
        );
        std::fs::write(&settings, r#"{"sharedConnectionPath": ""}"#).unwrap();
        assert_eq!(shared_connection_path(&settings), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
