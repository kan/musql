//! SQLite connections (#97).
//!
//! A SQLite profile names a database file (`MySqlConfig::file_path`) instead of a server,
//! so there is no SSH tunnel, TLS or password. Each tab keeps a connection of its own
//! (`SqliteSession`): the statements of a tab's "run all" share it, so a transaction, a
//! PRAGMA or a temp table carries from one statement to the next, and a tab's query can be
//! interrupted without touching another tab's.

use crate::{bytes_to_json, group_schema, QueryResult, SchemaColumn, SchemaInfo};
use rusqlite::{types::ValueRef, Connection, InterruptHandle, OpenFlags};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The database file the query window has open, with a connection per tab. Dropping it
/// closes them, which rolls back a transaction a tab left open.
pub struct SqliteSession {
    path: String,
    tabs: Mutex<HashMap<String, Arc<TabConnection>>>,
}

/// One tab's connection, and the handle that interrupts its running statement.
pub struct TabConnection {
    conn: Mutex<Connection>,
    interrupt: InterruptHandle,
}

impl SqliteSession {
    pub fn new(path: String) -> Self {
        SqliteSession {
            path,
            tabs: Mutex::new(HashMap::new()),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// The connection of the tab `tab`, opened on first use.
    pub fn tab(&self, tab: &str) -> Result<Arc<TabConnection>, String> {
        if let Some(existing) = lock(&self.tabs).get(tab) {
            return Ok(Arc::clone(existing));
        }
        // Opened outside the lock: the other tabs need not wait on the file.
        let conn = open(&self.path)?;
        let opened = Arc::new(TabConnection {
            interrupt: conn.get_interrupt_handle(),
            conn: Mutex::new(conn),
        });
        Ok(Arc::clone(
            lock(&self.tabs).entry(tab.to_owned()).or_insert(opened),
        ))
    }
}

impl TabConnection {
    /// The connection, once the tab's previous statement (if any) has finished.
    pub fn lock(&self) -> MutexGuard<'_, Connection> {
        lock(&self.conn)
    }

    /// Stops the statement running on this connection; a no-op when none is.
    pub fn interrupt(&self) {
        self.interrupt.interrupt();
    }
}

/// A panic while a lock was held leaves the data as it was (a map, or a connection that
/// SQLite keeps consistent), so the lock is taken over rather than refused.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// How long a statement waits for a lock held by another connection (another tab, or
/// another program) before it fails as busy.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Opens the database file at `path`. A file that does not exist is an error: SQLite
/// would otherwise create an empty database, and a mistyped path would look like an
/// empty one.
pub fn open(path: &str) -> Result<Connection, String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(
            "No database file is set for this connection. Choose one in the connection settings."
                .to_owned(),
        );
    }
    if !Path::new(path).is_file() {
        return Err(format!("Database file not found: {path}"));
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("Failed to open the SQLite database: {e}"))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| format!("Failed to open the SQLite database: {e}"))?;
    Ok(conn)
}

/// Opens the file and reads its schema: opening alone succeeds on any file, and SQLite
/// only finds out that it is not a database when it first reads it.
pub fn ping(path: &str) -> Result<(), String> {
    let conn = open(path)?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|e| format!("Failed to read the SQLite database: {e}"))?;
    Ok(())
}

/// Runs one statement. A statement that returns rows (SELECT, PRAGMA, `RETURNING`)
/// gives up to `limit` of them (0 = all); any other gives the number of rows changed.
pub fn run(conn: &Connection, sql: &str, limit: usize) -> Result<QueryResult, String> {
    let failed = |e: rusqlite::Error| format!("Query failed: {e}");
    let nothing = || QueryResult {
        columns: Vec::new(),
        rows: Vec::new(),
        affected_rows: 0,
    };
    let mut stmt = conn.prepare(sql).map_err(failed)?;
    // Only comments (what follows the last `;` of the editor, say): SQLite has no
    // statement to run, and stepping it would fail as a misuse.
    if stmt.expanded_sql().is_none() {
        return Ok(nothing());
    }
    if stmt.column_count() == 0 {
        let before = conn.total_changes();
        let changed = stmt.execute([]).map_err(failed)?;
        // `changes()` keeps the count of the connection's last INSERT, UPDATE or DELETE
        // through statements that change no rows (CREATE, BEGIN, ...).
        let changed_now = conn.total_changes() != before;
        return Ok(QueryResult {
            affected_rows: if changed_now { changed as u64 } else { 0 },
            ..nothing()
        });
    }
    let columns: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let mut rows = Vec::new();
    let mut result = stmt.query([]).map_err(failed)?;
    while let Some(row) = result.next().map_err(|e| format!("Row read failed: {e}"))? {
        let values = (0..columns.len())
            .map(|i| row.get_ref(i).map(value_to_json))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Row read failed: {e}"))?;
        rows.push(values);
        if limit > 0 && rows.len() >= limit {
            break;
        }
    }
    Ok(QueryResult {
        columns,
        rows,
        affected_rows: 0,
    })
}

/// A value as the result tables show it. Text and blobs go as MySQL's bytes do: as text
/// when they are UTF-8, else as hex (SQLite does not check that TEXT is UTF-8).
fn value_to_json(value: ValueRef<'_>) -> serde_json::Value {
    match value {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(v) => serde_json::json!(v),
        ValueRef::Real(v) => serde_json::json!(v),
        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => bytes_to_json(bytes),
    }
}

/// The tables and views of the file and their columns, for the AI prompts. `database`
/// only names the file in the prompt.
pub fn schema(path: &str, database: &str) -> Result<SchemaInfo, String> {
    let conn = open(path)?;
    let failed = |e: rusqlite::Error| format!("Schema query failed: {e}");
    // `sqlite_` names SQLite's own tables (sqlite_sequence, sqlite_stat1, ...).
    let mut stmt = conn
        .prepare(
            r#"SELECT m.name, p.name, p.type, p.pk, p."notnull"
               FROM sqlite_master AS m, pragma_table_info(m.name) AS p
               WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite\_%' ESCAPE '\'
               ORDER BY m.name, p.cid"#,
        )
        .map_err(failed)?;
    let columns = stmt
        .query_map([], |row| {
            let pk: i64 = row.get(3)?;
            let not_null: bool = row.get(4)?;
            Ok((
                row.get::<_, String>(0)?,
                SchemaColumn {
                    name: row.get(1)?,
                    data_type: row.get(2)?,
                    // The names MySQL's INFORMATION_SCHEMA uses, which the prompt reads.
                    column_key: if pk > 0 { "PRI" } else { "" }.to_owned(),
                    is_nullable: if not_null { "NO" } else { "YES" }.to_owned(),
                },
            ))
        })
        .map_err(failed)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(failed)?;
    Ok(group_schema(database, columns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    /// A database file of its own under the temp directory, removed when dropped.
    struct TempDb(PathBuf);

    impl TempDb {
        /// A path of its own, with no file there yet.
        fn at(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("musql-sqlite-test-{}-{name}", std::process::id()));
            let _ = std::fs::remove_file(&path);
            TempDb(path)
        }

        /// A database made by running `setup`.
        fn new(name: &str, setup: &str) -> Self {
            let db = Self::at(name);
            let conn = Connection::open(&db.0).unwrap();
            conn.execute_batch(setup).unwrap();
            db
        }

        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
    }

    impl Drop for TempDb {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn run_returns_rows_with_each_type() {
        let conn = Connection::open_in_memory().unwrap();
        let result = run(
            &conn,
            "SELECT 1 AS i, 1.5 AS r, 'あ' AS t, NULL AS n, x'00ff' AS b, x'6869' AS u",
            0,
        )
        .unwrap();
        assert_eq!(result.columns, ["i", "r", "t", "n", "b", "u"]);
        assert_eq!(
            result.rows,
            [vec![
                json!(1),
                json!(1.5),
                json!("あ"),
                json!(null),
                json!("0x00ff"),
                json!("hi")
            ]]
        );
    }

    #[test]
    fn run_stops_at_the_limit() {
        let conn = Connection::open_in_memory().unwrap();
        let sql = "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM n WHERE x < 10) \
                   SELECT x FROM n";
        assert_eq!(run(&conn, sql, 3).unwrap().rows.len(), 3);
        assert_eq!(run(&conn, sql, 0).unwrap().rows.len(), 10);
    }

    #[test]
    fn run_counts_changed_rows() {
        let conn = Connection::open_in_memory().unwrap();
        let ddl = run(&conn, "CREATE TABLE t (a INTEGER)", 0).unwrap();
        assert!(ddl.columns.is_empty());
        assert_eq!(ddl.affected_rows, 0);
        let insert = run(&conn, "INSERT INTO t VALUES (1), (2), (3)", 0).unwrap();
        assert_eq!(insert.affected_rows, 3);
        // The connection outlives the INSERT: a statement changing no rows still counts 0.
        assert_eq!(
            run(&conn, "CREATE TABLE u (b)", 0).unwrap().affected_rows,
            0
        );
        // RETURNING gives rows, not a count.
        let returning = run(&conn, "DELETE FROM t WHERE a > 1 RETURNING a", 0).unwrap();
        assert_eq!(returning.rows, [vec![json!(2)], vec![json!(3)]]);
    }

    // What follows the editor's last `;` can be a comment alone.
    #[test]
    fn run_takes_a_comment_alone_as_nothing_to_do() {
        let conn = Connection::open_in_memory().unwrap();
        let result = run(&conn, "-- done", 0).unwrap();
        assert!(result.columns.is_empty());
        assert_eq!(result.affected_rows, 0);
    }

    // A tab's statements run one by one but share its connection: a transaction or a temp
    // table carries over. Another tab has a connection of its own.
    #[test]
    fn a_tabs_statements_share_its_connection() {
        let db = TempDb::new("session.db", "CREATE TABLE t (a INTEGER);");
        let session = SqliteSession::new(db.path().to_owned());
        let tab = session.tab("sql-1").unwrap();
        let other = session.tab("sql-2").unwrap();
        let count = |conn: &Connection| run(conn, "SELECT count(*) FROM t", 0).unwrap().rows;

        run(&tab.lock(), "BEGIN", 0).unwrap();
        run(&tab.lock(), "INSERT INTO t VALUES (1)", 0).unwrap();
        run(&tab.lock(), "CREATE TEMP TABLE x (b)", 0).unwrap();
        assert!(run(&tab.lock(), "SELECT * FROM x", 0).is_ok());
        assert!(run(&other.lock(), "SELECT * FROM x", 0).is_err());
        assert_eq!(count(&other.lock()), [vec![json!(0)]]);
        run(&tab.lock(), "COMMIT", 0).unwrap();
        assert_eq!(count(&other.lock()), [vec![json!(1)]]);

        assert!(Arc::ptr_eq(&tab, &session.tab("sql-1").unwrap()));
    }

    #[test]
    fn run_refuses_more_than_one_statement() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(run(&conn, "SELECT 1; SELECT 2", 0).is_err());
    }

    #[test]
    fn open_does_not_create_a_missing_file() {
        let missing = TempDb::at("missing.db");
        let err = open(missing.path()).unwrap_err();
        assert!(err.contains("not found"), "{err}");
        assert!(!missing.0.exists());
        assert!(open("  ").unwrap_err().contains("No database file"));
    }

    #[test]
    fn ping_refuses_a_file_that_is_not_a_database() {
        let text = TempDb::at("not-a-db.txt");
        std::fs::write(&text.0, "this is not a database, just some text in a file").unwrap();
        assert!(ping(text.path()).is_err());
    }

    #[test]
    fn schema_lists_tables_and_views_but_not_sqlites_own() {
        let db = TempDb::new(
            "schema",
            "CREATE TABLE users (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);
             CREATE TABLE notes (body);
             CREATE VIEW names AS SELECT name FROM users;
             INSERT INTO users (name) VALUES ('a');",
        );
        ping(db.path()).unwrap();
        let schema = schema(db.path(), "app.db").unwrap();
        assert_eq!(schema.database, "app.db");
        let names: Vec<&str> = schema.tables.iter().map(|t| t.name.as_str()).collect();
        // sqlite_sequence exists (AUTOINCREMENT) but is SQLite's own.
        assert_eq!(names, ["names", "notes", "users"]);
        let users = &schema.tables[2];
        let id = &users.columns[0];
        assert_eq!(
            (
                id.name.as_str(),
                id.data_type.as_str(),
                id.column_key.as_str()
            ),
            ("id", "INTEGER", "PRI")
        );
        assert_eq!(users.columns[1].is_nullable, "NO");
        // A column declared without a type.
        assert_eq!(schema.tables[1].columns[0].data_type, "");
    }
}
