//! PostgreSQL connections (#96).
//!
//! The query window speaks to PostgreSQL much as it does to SQLite (#97): each tab keeps a
//! connection of its own (`PostgresSession`), so a transaction, a `SET` or a temp table
//! carries from one statement of a tab's "run all" to the next, and a tab's query can be
//! cancelled without touching another tab's. A connection is to one database (PostgreSQL
//! has no `USE`), so choosing another database makes a new session.
//!
//! Statements go through the simple query protocol, which returns every value as text, as
//! the MySQL client's text protocol does: the result tables show what the server printed.

use crate::{
    group_schema, unknown_ssl_mode_error, MySqlConfig, QueryResult, SchemaColumn, SchemaInfo,
    SslMode, CONNECT_TIMEOUT,
};
use postgres::error::ErrorPosition;
use postgres::{CancelToken, Client, Config, SimpleQueryMessage};
use postgres_native_tls::MakeTlsConnector;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::net::IpAddr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The server and database the query window has open, with a connection per tab.
pub struct PostgresSession {
    config: Config,
    tls: MakeTlsConnector,
    tabs: Mutex<HashMap<String, Arc<TabConnection>>>,
}

/// One tab's connection, and the token that cancels its running statement.
pub struct TabConnection {
    /// `None` only while the connection is being dropped (see `Drop`).
    client: Mutex<Option<Client>>,
    cancel: CancelToken,
    tls: MakeTlsConnector,
}

impl PostgresSession {
    /// A session on the server at `host:port` (the profile's server, or the local end of
    /// an SSH tunnel to it). Nothing connects until a tab first asks (`tab`).
    pub fn new(mysql: &MySqlConfig, host: &str, port: u16) -> Result<Self, String> {
        Ok(PostgresSession {
            config: config(mysql, host, port)?,
            tls: tls(mysql)?,
            tabs: Mutex::new(HashMap::new()),
        })
    }

    /// The connection of the tab `tab`, opened on first use.
    ///
    /// When the server has closed it (a restart, an idle timeout), `Reopen::Quietly` opens
    /// a new one: right for the window's own queries, which keep no state. A SQL tab's
    /// (`Reopen::AfterTelling`) fails once instead, and the tab's next statement opens a
    /// new one: a transaction the tab had open is gone, and its later statements would
    /// otherwise each commit on their own without anyone noticing.
    pub fn tab(&self, tab: &str, reopen: Reopen) -> Result<Arc<TabConnection>, String> {
        {
            let mut tabs = lock(&self.tabs);
            if let Some(existing) = tabs.get(tab) {
                if !existing.is_closed() {
                    return Ok(Arc::clone(existing));
                }
                tabs.remove(tab);
                if reopen == Reopen::AfterTelling {
                    return Err(
                        "The connection to PostgreSQL was closed (by the server, or a \
                         timeout), with any transaction that was open. Run the statement \
                         again to reconnect."
                            .to_owned(),
                    );
                }
            }
        }
        // Connected outside the lock: the other tabs need not wait on the server.
        let client = self
            .config
            .connect(self.tls.clone())
            .map_err(|e| format!("Failed to connect PostgreSQL: {}", describe(&e, "")))?;
        let opened = Arc::new(TabConnection {
            cancel: client.cancel_token(),
            client: Mutex::new(Some(client)),
            tls: self.tls.clone(),
        });
        lock(&self.tabs).insert(tab.to_owned(), Arc::clone(&opened));
        Ok(opened)
    }

    /// Lets go of the tab's connection (the tab was closed): it closes once a statement
    /// still running on it ends, which rolls back a transaction left open. Each connection
    /// is a server process, so a closed tab's should not wait for the window to close.
    pub fn close(&self, tab: &str) {
        lock(&self.tabs).remove(tab);
    }
}

/// What `PostgresSession::tab` does with a connection the server has closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reopen {
    Quietly,
    AfterTelling,
}

/// A tab's connection, held while one statement runs on it.
pub struct TabGuard<'a>(MutexGuard<'a, Option<Client>>);

impl TabConnection {
    /// The connection, once the tab's previous statement (if any) has finished.
    pub fn lock(&self) -> TabGuard<'_> {
        TabGuard(lock(&self.client))
    }

    /// Asks the server to cancel the statement running on this connection; a no-op when
    /// none is. Blocks while it reaches the server.
    pub fn cancel(&self) -> Result<(), String> {
        self.cancel
            .cancel_query(self.tls.clone())
            .map_err(|e| format!("Failed to cancel query: {e}"))
    }

    /// Whether the server has closed the connection. A connection that is running a
    /// statement is taken as open.
    fn is_closed(&self) -> bool {
        match self.client.try_lock() {
            Ok(guard) => guard.as_ref().is_none_or(Client::is_closed),
            Err(std::sync::TryLockError::Poisoned(e)) => {
                e.into_inner().as_ref().is_none_or(Client::is_closed)
            }
            Err(std::sync::TryLockError::WouldBlock) => false,
        }
    }
}

impl TabGuard<'_> {
    /// Runs one statement.
    pub fn run(&mut self, sql: &str, limit: usize) -> Result<QueryResult, String> {
        run(self.client()?, sql, limit)
    }

    /// The tables and views of the database, for the AI prompts.
    pub fn schema(&mut self, database: &str) -> Result<SchemaInfo, String> {
        schema(self.client()?, database)
    }

    fn client(&mut self) -> Result<&mut Client, String> {
        self.0
            .as_mut()
            .ok_or_else(|| "The connection is closed.".to_owned())
    }
}

impl Drop for TabConnection {
    // Closing a `postgres::Client` blocks on a runtime of its own, which panics on a
    // thread that is running async tasks, as the last reference can be dropped on one
    // (a new connection replacing this session's, from an async command). A thread of
    // its own is never one.
    fn drop(&mut self) {
        let client = self
            .client
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(client) = client {
            std::thread::spawn(move || drop(client));
        }
    }
}

/// A panic while a lock was held leaves the data as it was (a map, or a connection whose
/// statement either finished or failed), so the lock is taken over rather than refused.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How to reach the server. The certificate is checked against the profile's host, also
/// through an SSH tunnel (`host` is then 127.0.0.1): the host names the server and
/// `hostaddr` says where to connect, so `VERIFY_IDENTITY` works through SSH, unlike MySQL
/// (#133).
fn config(mysql: &MySqlConfig, host: &str, port: u16) -> Result<Config, String> {
    let mut config = Config::new();
    let name = mysql.host.trim();
    config
        .host(name)
        .port(port)
        .user(&mysql.username)
        .password(&mysql.password)
        .connect_timeout(CONNECT_TIMEOUT)
        .application_name("muSQL")
        // NOTICE and WARNING messages (`RAISE NOTICE`, ...) are logged at info by default;
        // they can carry the data a statement works on, which the log must not hold.
        .notice_callback(|_| {})
        .ssl_mode(ssl_mode(&mysql.ssl_mode)?);
    // Without a tunnel `host` is the profile's own, not yet trimmed.
    if host.trim() != name {
        let addr: IpAddr = host
            .parse()
            .map_err(|_| format!("Not an IP address to connect to: {host}"))?;
        config.hostaddr(addr);
    }
    // Without one the server takes the user's name, as psql does.
    if let Some(db) = mysql.database.as_deref().map(str::trim) {
        if !db.is_empty() {
            config.dbname(db);
        }
    }
    Ok(config)
}

fn ssl_mode(mode: &SslMode) -> Result<postgres::config::SslMode, String> {
    match mode {
        SslMode::Disabled => Ok(postgres::config::SslMode::Disable),
        SslMode::Required | SslMode::VerifyCa | SslMode::VerifyIdentity => {
            Ok(postgres::config::SslMode::Require)
        }
        SslMode::Unknown(name) => Err(unknown_ssl_mode_error(name)),
    }
}

/// The TLS of the connection, with the same meaning of the SSL modes as MySQL's: REQUIRED
/// checks nothing, VERIFY_CA the certificate but not the host name, VERIFY_IDENTITY both.
/// The CA certificate is trusted besides the system's, as the mysql crate does.
fn tls(mysql: &MySqlConfig) -> Result<MakeTlsConnector, String> {
    let mut builder = native_tls::TlsConnector::builder();
    match &mysql.ssl_mode {
        SslMode::Disabled => {}
        SslMode::Required => {
            builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }
        SslMode::VerifyCa => {
            builder.danger_accept_invalid_hostnames(true);
        }
        SslMode::VerifyIdentity => {}
        SslMode::Unknown(name) => return Err(unknown_ssl_mode_error(name)),
    }
    if matches!(mysql.ssl_mode, SslMode::VerifyCa | SslMode::VerifyIdentity) {
        if let Some(path) = mysql.ca_cert_path() {
            for cert in ca_certificates(path)? {
                builder.add_root_certificate(cert);
            }
        }
    }
    builder
        .build()
        .map(MakeTlsConnector::new)
        .map_err(|e| format!("Failed to set up TLS: {e}"))
}

/// The certificates of a CA file: PEM (one or more), or a single DER certificate.
fn ca_certificates(path: &str) -> Result<Vec<native_tls::Certificate>, String> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    let data =
        std::fs::read(path).map_err(|e| format!("Failed to read the CA certificate: {e}"))?;
    let invalid = |e: &dyn std::fmt::Display| format!("Invalid CA certificate: {e}");
    let mut certs = Vec::new();
    for der in CertificateDer::pem_slice_iter(&data) {
        let der = der.map_err(|e| invalid(&e))?;
        certs.push(native_tls::Certificate::from_der(&der).map_err(|e| invalid(&e))?);
    }
    if certs.is_empty() {
        certs.push(native_tls::Certificate::from_der(&data).map_err(|e| invalid(&e))?);
    }
    Ok(certs)
}

/// Connects and runs `SELECT 1`: the connection test.
pub fn ping(mysql: &MySqlConfig, host: &str, port: u16) -> Result<(), String> {
    PostgresSession::new(mysql, host, port)?
        .tab("ping", Reopen::Quietly)?
        .lock()
        .run("SELECT 1", 1)
        .map(|_| ())
}

/// Runs `sql` and gives its last statement's outcome: up to `limit` rows (0 = all) of a
/// statement that returns them, else the number of rows it changed. The window sends one
/// statement at a time.
pub fn run(client: &mut Client, sql: &str, limit: usize) -> Result<QueryResult, String> {
    let messages = client
        .simple_query(sql)
        .map_err(|e| format!("Query failed: {}", describe(&e, sql)))?;
    let mut result = QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        affected_rows: 0,
    };
    // Whether the statement being read returns rows: its count is then the rows', not
    // a number of changed ones.
    let mut returns_rows = false;
    for message in messages {
        match message {
            SimpleQueryMessage::RowDescription(columns) => {
                result.columns = columns.iter().map(|c| c.name().to_owned()).collect();
                result.rows.clear();
                result.affected_rows = 0;
                returns_rows = true;
            }
            SimpleQueryMessage::Row(row) => {
                if limit == 0 || result.rows.len() < limit {
                    result.rows.push(
                        (0..row.len())
                            .map(|i| row.get(i).map_or(serde_json::Value::Null, Into::into))
                            .collect(),
                    );
                }
            }
            SimpleQueryMessage::CommandComplete(count) => {
                if !returns_rows {
                    result.columns.clear();
                    result.rows.clear();
                    result.affected_rows = count;
                }
                returns_rows = false;
            }
            _ => {}
        }
    }
    Ok(result)
}

/// An error as psql prints it: the server's message, the line of `sql` it points at, and
/// its detail and hint. A failure that is not the server's (the network, the TLS) gives
/// the client's message.
fn describe(error: &postgres::Error, sql: &str) -> String {
    let Some(db) = error.as_db_error() else {
        // The client's message names the step ("error performing TLS handshake"); the
        // reason is in its sources.
        let mut text = error.to_string();
        let mut source = std::error::Error::source(error);
        while let Some(cause) = source {
            let cause_text = cause.to_string();
            if !text.ends_with(&cause_text) {
                let _ = write!(text, ": {cause_text}");
            }
            source = cause.source();
        }
        return text;
    };
    let mut text = format!("{}: {}", db.severity(), db.message());
    if let Some(ErrorPosition::Original(position)) = db.position() {
        if let Some((line, content)) = line_at(sql, *position) {
            let _ = write!(text, "\nLINE {line}: {content}");
        }
    }
    if let Some(detail) = db.detail() {
        let _ = write!(text, "\nDETAIL: {detail}");
    }
    if let Some(hint) = db.hint() {
        let _ = write!(text, "\nHINT: {hint}");
    }
    text
}

/// The 1-based line of `sql` that holds its `position`-th character (1-based, as the
/// server counts), and that line's text.
fn line_at(sql: &str, position: u32) -> Option<(usize, &str)> {
    let index = usize::try_from(position).ok()?.checked_sub(1)?;
    let offset = sql.char_indices().nth(index).map(|(i, _)| i)?;
    let line = sql[..offset].matches('\n').count() + 1;
    let start = sql[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = sql[offset..].find('\n').map_or(sql.len(), |i| offset + i);
    Some((line, sql[start..end].trim_end_matches('\r')))
}

/// Schemas that are PostgreSQL's own, left out of the lists: `pg_catalog`, `pg_toast`,
/// the temp schemas (`pg_temp_N`), and `information_schema`. `left` rather than LIKE: a
/// LIKE pattern's backslash would depend on `standard_conforming_strings`.
const USER_SCHEMAS: &str = "left(n.nspname, 3) <> 'pg_' AND n.nspname <> 'information_schema'";

/// The tables and views of the database and their columns, for the AI prompts. A table
/// outside `public` is named with its schema. `database` names the database in the prompt.
pub fn schema(client: &mut Client, database: &str) -> Result<SchemaInfo, String> {
    // The kinds the table list shows: tables, views, materialized views, partitioned and
    // foreign tables. A partition is left to its parent. Key: the first constraint kind of
    // the column, in the names MySQL's INFORMATION_SCHEMA uses, which the prompt reads
    // (PRI for the primary key, MUL for a foreign key, UNI for a unique one).
    let sql = format!(
        "SELECT n.nspname, c.relname, a.attname, format_type(a.atttypid, a.atttypmod), \
           COALESCE((SELECT CASE k.contype WHEN 'p' THEN 'PRI' WHEN 'f' THEN 'MUL' ELSE 'UNI' END \
             FROM pg_constraint k \
             WHERE k.conrelid = c.oid AND k.contype IN ('p', 'f', 'u') AND a.attnum = ANY (k.conkey) \
             ORDER BY k.contype = 'p' DESC, k.contype = 'f' DESC LIMIT 1), ''), \
           CASE WHEN a.attnotnull THEN 'NO' ELSE 'YES' END \
         FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
         WHERE c.relkind IN ('r', 'v', 'm', 'p', 'f') AND NOT c.relispartition AND {USER_SCHEMAS} \
         ORDER BY n.nspname, c.relname, a.attnum"
    );
    let messages = client
        .simple_query(&sql)
        .map_err(|e| format!("Schema query failed: {}", describe(&e, &sql)))?;
    let text = |row: &postgres::SimpleQueryRow, i: usize| row.get(i).unwrap_or("").to_owned();
    let columns = messages
        .iter()
        .filter_map(|message| match message {
            SimpleQueryMessage::Row(row) => Some(row),
            _ => None,
        })
        .map(|row| {
            let (schema, table) = (text(row, 0), text(row, 1));
            let name = if schema == "public" {
                table
            } else {
                format!("{schema}.{table}")
            };
            let column = SchemaColumn {
                name: text(row, 2),
                data_type: text(row, 3),
                column_key: text(row, 4),
                is_nullable: text(row, 5),
            };
            (name, column)
        })
        .collect();
    Ok(group_schema(database, columns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use postgres::config::Host;

    fn profile(host: &str, ssl_mode: SslMode) -> MySqlConfig {
        serde_json::from_value(serde_json::json!({
            "host": host,
            "port": 5432,
            "database": "app",
            "username": "postgres",
            "ssl_mode": String::from(ssl_mode),
        }))
        .unwrap()
    }

    #[test]
    fn config_connects_to_the_profiles_server() {
        let config = config(
            &profile("db.example.com", SslMode::Disabled),
            "db.example.com",
            5432,
        )
        .unwrap();
        assert_eq!(config.get_hosts(), [Host::Tcp("db.example.com".to_owned())]);
        assert!(config.get_hostaddrs().is_empty());
        assert_eq!(config.get_ports(), [5432]);
        assert_eq!(config.get_dbname(), Some("app"));
        assert_eq!(config.get_ssl_mode(), postgres::config::SslMode::Disable);
    }

    // Through an SSH tunnel the connection goes to 127.0.0.1, but the certificate is
    // still checked against the profile's host (#133).
    #[test]
    fn config_through_a_tunnel_keeps_the_host_name_for_tls() {
        let config = config(
            &profile("db.example.com", SslMode::VerifyIdentity),
            "127.0.0.1",
            50123,
        )
        .unwrap();
        assert_eq!(config.get_hosts(), [Host::Tcp("db.example.com".to_owned())]);
        assert_eq!(
            config.get_hostaddrs(),
            ["127.0.0.1".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(config.get_ports(), [50123]);
        assert_eq!(config.get_ssl_mode(), postgres::config::SslMode::Require);
    }

    #[test]
    fn config_without_a_database_leaves_it_to_the_server() {
        let mut mysql = profile("h", SslMode::Disabled);
        mysql.database = Some("  ".to_owned());
        assert_eq!(config(&mysql, "h", 5432).unwrap().get_dbname(), None);
    }

    // As MySQL's (#131): an unknown mode is refused, never taken as no TLS.
    #[test]
    fn an_unknown_ssl_mode_is_refused() {
        let mysql = profile("h", SslMode::Unknown("PREFER".to_owned()));
        assert!(config(&mysql, "h", 5432).is_err());
        assert!(tls(&mysql).is_err());
    }

    // The table list (ui/query.js) and the AI schema leave out the same schemas: the
    // condition is written in both languages.
    #[test]
    fn the_table_list_leaves_out_the_same_schemas() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/query.js");
        let script = std::fs::read_to_string(&path).unwrap();
        let declared = format!("const PG_USER_SCHEMAS = \"{USER_SCHEMAS}\";");
        assert!(script.contains(&declared), "ui/query.js lacks {declared}");
    }

    #[test]
    fn line_at_finds_the_line_of_a_character() {
        let sql = "SELECT 1\r\nFROMM t\nWHERE x";
        // The 11th character is the F of FROMM (the \r and \n count as one each).
        assert_eq!(line_at(sql, 11), Some((2, "FROMM t")));
        assert_eq!(line_at(sql, 1), Some((1, "SELECT 1")));
        assert_eq!(line_at(sql, 20), Some((3, "WHERE x")));
        assert_eq!(line_at(sql, 0), None);
        assert_eq!(line_at(sql, 99), None);
        // Characters, not bytes.
        assert_eq!(line_at("SELECT 'あ'\nX", 12), Some((2, "X")));
    }
}
