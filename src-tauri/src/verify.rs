//! Opt-in checks against real servers (#117).
//!
//! These exist for one situation: a dependency that talks to the outside world was
//! updated (russh, the mysql crate, bollard) and it should be confirmed that connecting
//! still works, without starting the GUI and clicking through it.
//!
//! They are `#[ignore]` tests, like the 1Password ones, so `just test` and CI never run
//! them. Each one is driven by environment variables and **prints "skipped" and passes
//! when its variables are not set**, so `just test-ignored` stays green on a machine
//! that has nothing configured.
//!
//! ```text
//! just verify-ssh      # MUSQL_VERIFY_SSH_HOST, …
//! just verify-mysql    # MUSQL_VERIFY_MYSQL_HOST, … (through the SSH host if set)
//! just verify-docker   # MUSQL_VERIFY_DOCKER=1
//! just verify-redis    # MUSQL_VERIFY_REDIS_HOST, … (writes, then deletes, musql:verify:* keys)
//! ```
//!
//! Tests rather than `src/bin/verify_*.rs` because muSQL is a binary-only crate: a
//! separate binary could not call the connection code in `main.rs` without first
//! splitting it into a library. As a test module this calls the very functions the app
//! uses. It also runs while `just dev` holds `musql.exe`.
//!
//! Nothing here goes in the repository: hosts and credentials come from the environment,
//! and passwords are never printed.

use super::*;

/// A non-empty environment variable.
fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn env_or(name: &str, default: &str) -> String {
    env(name).unwrap_or_else(|| default.to_owned())
}

fn env_port(name: &str, default: u16) -> u16 {
    match env(name) {
        Some(v) => v
            .parse()
            .unwrap_or_else(|_| panic!("{name} is not a port number: {v}")),
        None => default,
    }
}

/// The SSH jump host from `MUSQL_VERIFY_SSH_*`, or `None` when no host is set.
///
/// | Variable | Default |
/// |---|---|
/// | `MUSQL_VERIFY_SSH_HOST` | (required) |
/// | `MUSQL_VERIFY_SSH_PORT` | 22 |
/// | `MUSQL_VERIFY_SSH_USER` | (required) |
/// | `MUSQL_VERIFY_SSH_AUTH` | `key` (or `password`) |
/// | `MUSQL_VERIFY_SSH_KEY` | none: agent, then the default keys |
/// | `MUSQL_VERIFY_SSH_PASSPHRASE` | empty |
/// | `MUSQL_VERIFY_SSH_PASSWORD` | empty |
fn ssh_from_env() -> Option<SshConfig> {
    let host = env("MUSQL_VERIFY_SSH_HOST")?;
    let username =
        env("MUSQL_VERIFY_SSH_USER").expect("MUSQL_VERIFY_SSH_USER is required with _SSH_HOST");
    Some(SshConfig {
        enabled: true,
        host,
        port: env_port("MUSQL_VERIFY_SSH_PORT", 22),
        username,
        private_key_path: env("MUSQL_VERIFY_SSH_KEY"),
        config_host: None,
        passphrase: env_or("MUSQL_VERIFY_SSH_PASSPHRASE", ""),
        auth_method: env_or("MUSQL_VERIFY_SSH_AUTH", "key"),
        ssh_password: env_or("MUSQL_VERIFY_SSH_PASSWORD", ""),
        save_ssh_password: false,
        save_ssh_passphrase: false,
        op_passphrase_ref: None,
        op_password_ref: None,
    })
}

/// The MySQL server from `MUSQL_VERIFY_MYSQL_*`, or `None` when no host is set.
///
/// | Variable | Default |
/// |---|---|
/// | `MUSQL_VERIFY_MYSQL_HOST` | (required) |
/// | `MUSQL_VERIFY_MYSQL_PORT` | 3306 |
/// | `MUSQL_VERIFY_MYSQL_USER` | `root` |
/// | `MUSQL_VERIFY_MYSQL_PASSWORD` | empty |
/// | `MUSQL_VERIFY_MYSQL_DATABASE` | none |
/// | `MUSQL_VERIFY_MYSQL_SSL_MODE` | `DISABLED` |
/// | `MUSQL_VERIFY_MYSQL_CA_CERT` | none |
fn mysql_from_env() -> Option<MySqlConfig> {
    let host = env("MUSQL_VERIFY_MYSQL_HOST")?;
    Some(MySqlConfig {
        host,
        port: env_port("MUSQL_VERIFY_MYSQL_PORT", 3306),
        database: env("MUSQL_VERIFY_MYSQL_DATABASE"),
        username: env_or("MUSQL_VERIFY_MYSQL_USER", "root"),
        password: env_or("MUSQL_VERIFY_MYSQL_PASSWORD", ""),
        // Read as a saved profile would be; a misspelling becomes `Unknown`, which the
        // connection test then refuses.
        ssl_mode: env_or("MUSQL_VERIFY_MYSQL_SSL_MODE", "DISABLED").into(),
        tls_ca_cert_path: env("MUSQL_VERIFY_MYSQL_CA_CERT"),
        save_password: false,
        op_ref: None,
        tls_enabled: false,
        tls_skip_verify: false,
    })
}

/// Authenticates against the SSH host and opens the tunnel listener. The forward target
/// is not contacted until something connects through the tunnel, so this checks the
/// SSH side on its own.
#[test]
#[ignore]
fn verify_ssh() {
    let Some(ssh) = ssh_from_env() else {
        println!("verify_ssh: skipped (set MUSQL_VERIFY_SSH_HOST and MUSQL_VERIFY_SSH_USER)");
        return;
    };
    println!(
        "verify_ssh: {}@{}:{} (auth: {})",
        ssh.username, ssh.host, ssh.port, ssh.auth_method
    );
    let tunnel = tauri::async_runtime::block_on(start_ssh_tunnel(&ssh, "127.0.0.1", 3306))
        .unwrap_or_else(|e| panic!("SSH connection failed: {e}"));
    println!(
        "verify_ssh: ok, authenticated; tunnel listening on 127.0.0.1:{}",
        tunnel.local_port
    );
}

/// Connects to MySQL, through the SSH host when one is configured.
///
/// First through `run_connection_test`, the code behind the settings window's test
/// button. Then once more to report what the server negotiated, since "it connected"
/// does not say whether the SSL mode took effect.
#[test]
#[ignore]
fn verify_mysql() {
    let Some(mysql) = mysql_from_env() else {
        println!("verify_mysql: skipped (set MUSQL_VERIFY_MYSQL_HOST)");
        return;
    };
    let ssh = ssh_from_env();
    println!(
        "verify_mysql: {}@{}:{} (ssl: {:?}){}",
        mysql.username,
        mysql.host,
        mysql.port,
        mysql.ssl_mode,
        match &ssh {
            Some(s) => format!(" via ssh {}@{}:{}", s.username, s.host, s.port),
            None => String::new(),
        }
    );

    let request = ConnectionRequest {
        engine: Engine::Mysql,
        mysql: mysql.clone(),
        ssh: ssh.clone(),
    };
    let message = tauri::async_runtime::block_on(run_connection_test(request, None))
        .unwrap_or_else(|e| panic!("connection test failed: {e}"));
    println!("verify_mysql: connection test: {message}");

    // The tunnel must stay alive for as long as the pool is used.
    let tunnel = ssh.as_ref().map(|s| {
        tauri::async_runtime::block_on(start_ssh_tunnel(s, &mysql.host, mysql.port))
            .unwrap_or_else(|e| panic!("SSH tunnel failed: {e}"))
    });
    let (host, port) = match &tunnel {
        Some(t) => ("127.0.0.1".to_owned(), t.local_port),
        None => (mysql.host.clone(), mysql.port),
    };
    let opts = build_opts(&mysql, &host, port).unwrap_or_else(|e| panic!("{e}"));
    let pool = Pool::new(opts).unwrap_or_else(|e| panic!("failed to build pool: {e}"));
    let mut conn = pool
        .get_conn()
        .unwrap_or_else(|e| panic!("failed to connect: {e}"));
    let version: Option<String> = conn
        .query_first("SELECT VERSION()")
        .unwrap_or_else(|e| panic!("SELECT VERSION() failed: {e}"));
    let cipher: Option<(String, String)> = conn
        .query_first("SHOW SESSION STATUS LIKE 'Ssl_cipher'")
        .unwrap_or_else(|e| panic!("SHOW STATUS failed: {e}"));
    let cipher = cipher.map(|(_, value)| value).unwrap_or_default();
    println!(
        "verify_mysql: ok, server {}; TLS cipher: {}",
        version.unwrap_or_else(|| "(unknown)".to_owned()),
        if cipher.is_empty() {
            "(none, unencrypted)"
        } else {
            &cipher
        }
    );

    // The mode asked for must be the one in effect, in both directions.
    let wants_tls = mysql.ssl_mode != SslMode::Disabled;
    assert_eq!(
        !cipher.is_empty(),
        wants_tls,
        "ssl_mode is {:?} but the session is {}",
        mysql.ssl_mode,
        if cipher.is_empty() {
            "unencrypted"
        } else {
            "encrypted"
        }
    );
}

/// Shows a real toast and waits for it to be clicked (#113).
///
/// A toast cannot be checked from code: whether a banner appears, and whether the click
/// comes back, is only visible on the desktop. Run this, click the banner within the
/// time limit, and it reports what happened. It uses the dev AUMID and the dev Start
/// Menu shortcut, the same ones `just dev` uses.
///
/// Set `MUSQL_VERIFY_TOAST=1` to run it; a number is taken as the seconds to wait
/// (anything else, 30).
#[test]
#[ignore]
fn verify_toast() {
    let Some(setting) = env("MUSQL_VERIFY_TOAST") else {
        println!("verify_toast: skipped (set MUSQL_VERIFY_TOAST=1)");
        return;
    };
    let wait = Duration::from_secs(setting.parse().ok().filter(|s| *s > 1).unwrap_or(30));
    // The identifier `just dev` runs with (tauri.dev.conf.json).
    let aumid = "jp.co.communitylinks.musql.debug";

    use std::sync::mpsc::RecvTimeoutError;
    // COM has to be initialised on the thread that shows the toast, and the test
    // harness thread is not ours to initialise.
    //
    // **The wait happens on that same thread.** The click handler belongs to the
    // apartment of the thread that showed the toast; if the thread ended first, the
    // handler would be released and the click would have nowhere to go.
    let clicked = std::thread::spawn(move || {
        assert!(toast::init_com(), "CoInitializeEx failed");
        let (clicked_tx, clicked_rx) = std::sync::mpsc::channel::<()>();
        // The dev shortcut should point at the dev app, not at this test binary
        // (`target/debug/deps/musql-<hash>.exe`, whose name changes with every build).
        let this_exe = std::env::current_exe().expect("current_exe");
        let dev_exe = this_exe
            .parent()
            .and_then(|deps| deps.parent())
            .map(|debug| debug.join("musql.exe"))
            .filter(|exe| exe.is_file())
            .unwrap_or(this_exe);
        unsafe {
            toast::ensure_shortcut(aumid, &dev_exe)
                .unwrap_or_else(|e| panic!("preparing the shortcut failed: {e}"));
            toast::show(
                aumid,
                "muSQL verify_toast",
                "Click this banner while it is on screen.",
                move || {
                    let _ = clicked_tx.send(());
                },
            )
            .unwrap_or_else(|e| panic!("showing the toast failed: {e}"));
        }
        println!(
            "verify_toast: toast shown (shortcut: {}). Click the banner within {}s...",
            toast::link_name(aumid),
            wait.as_secs()
        );
        clicked_rx.recv_timeout(wait)
    })
    .join()
    .expect("the toast thread panicked (see the message above)");

    match clicked {
        Ok(()) => println!("verify_toast: ok, the click came back"),
        Err(RecvTimeoutError::Timeout) => panic!(
            "no click within {}s. If no banner appeared, check Focus Assist / Do Not Disturb \
             and that notifications are allowed for \"muSQL (dev)\" in Windows settings.",
            wait.as_secs()
        ),
        // The handler was dropped without being called: Windows let go of the toast.
        Err(RecvTimeoutError::Disconnected) => {
            panic!("the click handler was released before any click could arrive")
        }
    }
}

/// Reaches the Docker API and lists the MySQL containers muSQL would offer.
#[cfg(feature = "docker")]
#[test]
#[ignore]
fn verify_docker() {
    if env("MUSQL_VERIFY_DOCKER").is_none() {
        println!("verify_docker: skipped (set MUSQL_VERIFY_DOCKER=1)");
        return;
    }
    let containers = tauri::async_runtime::block_on(async {
        let docker = connect_docker().await?;
        docker::discovery::discover_containers(&docker).await
    })
    .unwrap_or_else(|e| panic!("Docker failed: {e}"));
    println!(
        "verify_docker: ok, API reachable; {} MySQL / Redis container(s)",
        containers.len()
    );
    for c in &containers {
        println!(
            "verify_docker:   {} ({}, {}) port {} -> host {}",
            c.name,
            c.engine.display_name(),
            c.image,
            c.port,
            c.host_port.map_or_else(
                || "(not published, needs a tunnel)".to_owned(),
                |p| p.to_string()
            )
        );
    }
}

/// Runs the Redis window's calls against a real server (#98): the key list, each type's
/// view, and the CLI.
///
/// **Writes to the server**: one key of each type under `musql:verify:`, deleted again at
/// the end. Point it at a scratch server.
///
/// | Variable | Default |
/// |---|---|
/// | `MUSQL_VERIFY_REDIS_HOST` | (required) |
/// | `MUSQL_VERIFY_REDIS_PORT` | 6379 |
/// | `MUSQL_VERIFY_REDIS_USER` | empty (the default user) |
/// | `MUSQL_VERIFY_REDIS_PASSWORD` | empty |
/// | `MUSQL_VERIFY_REDIS_DB` | 0 |
/// | `MUSQL_VERIFY_REDIS_SSL_MODE` | `DISABLED` |
#[test]
#[ignore]
fn verify_redis() {
    let Some(host) = env("MUSQL_VERIFY_REDIS_HOST") else {
        println!("verify_redis: skipped (set MUSQL_VERIFY_REDIS_HOST)");
        return;
    };
    let port = env_port("MUSQL_VERIFY_REDIS_PORT", 6379);
    let mysql = MySqlConfig {
        host: host.clone(),
        port,
        database: env("MUSQL_VERIFY_REDIS_DB"),
        username: env_or("MUSQL_VERIFY_REDIS_USER", ""),
        password: env_or("MUSQL_VERIFY_REDIS_PASSWORD", ""),
        ssl_mode: env_or("MUSQL_VERIFY_REDIS_SSL_MODE", "DISABLED").into(),
        tls_ca_cert_path: None,
        save_password: false,
        op_ref: None,
        tls_enabled: false,
        tls_skip_verify: false,
    };
    let client = redis_client::client(&mysql, &host, port).unwrap_or_else(|e| panic!("{e}"));
    let mut session =
        redis_client::RedisSession::open(client, None).unwrap_or_else(|e| panic!("{e}"));
    let version = redis_client::server_version(&mut session).unwrap_or_else(|e| panic!("{e}"));
    println!("verify_redis: connected to Redis {version} at {host}:{port}");

    // Runs a line that needs no confirmation.
    let run = |s: &mut redis_client::RedisSession, line: &str| {
        redis_client::command(s, line, false)
            .unwrap_or_else(|e| panic!("{line}: {e}"))
            .reply
    };
    // (type, the command that writes it, its arguments after the key). The key is
    // `musql:verify:<type>`.
    let seeds = [
        ("string", "SET", r#""{\"a\":1}""#),
        ("hash", "HSET", "f1 v1 f2 v2"),
        ("list", "RPUSH", "x y z"),
        ("set", "SADD", "m1 m2"),
        ("zset", "ZADD", "1 a 2 b"),
        ("stream", "XADD", "* user ann"),
    ];
    let keys: Vec<(String, &str)> = seeds
        .iter()
        .map(|(kind, _, _)| (format!("musql:verify:{kind}"), *kind))
        .collect();
    for ((key, _), (_, command, args)) in keys.iter().zip(&seeds) {
        run(&mut session, &format!("{command} {key} {args}"));
    }

    // The key list, through as many calls as it takes.
    let mut found = Vec::new();
    let mut cursor = 0;
    loop {
        let page = redis_client::scan(&mut session, cursor, "musql:verify:*", || true)
            .unwrap_or_else(|e| panic!("SCAN: {e}"));
        let page = serde_json::to_value(page).unwrap();
        for entry in page["keys"].as_array().unwrap() {
            found.push((
                entry["key"].as_str().unwrap().to_owned(),
                entry["kind"].as_str().unwrap().to_owned(),
            ));
        }
        cursor = page["cursor"].as_str().unwrap().parse().unwrap();
        if cursor == 0 {
            break;
        }
    }
    found.sort();
    let mut expected: Vec<(String, String)> = keys
        .iter()
        .map(|(key, kind)| (key.clone(), (*kind).to_owned()))
        .collect();
    expected.sort();
    assert_eq!(found, expected, "the key list");
    println!(
        "verify_redis: ok, SCAN found the {} seeded keys",
        found.len()
    );

    // Each type's view.
    for (key, kind) in &keys {
        let view =
            redis_client::key_view(&mut session, key.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        let view = serde_json::to_value(view).unwrap();
        assert_eq!(view["kind"], *kind, "{key}");
        println!(
            "verify_redis:   {kind}: size {}, {} row(s), text {}",
            view["size"],
            view["rows"].as_array().unwrap().len(),
            view["text"]
        );
    }

    // The CLI: a reply, a server error shown as output, a refused command, and a
    // dangerous one held for confirmation.
    println!(
        "verify_redis:   LRANGE -> {}",
        run(&mut session, "LRANGE musql:verify:list 0 -1").replace('\n', " | ")
    );
    let wrong_type = run(&mut session, "LPUSH musql:verify:string x");
    assert!(wrong_type.starts_with("(error)"), "{wrong_type}");
    println!("verify_redis:   wrong type -> {wrong_type}");
    assert!(redis_client::command(&mut session, "SUBSCRIBE musql:verify", false).is_err());
    let held = redis_client::command(&mut session, r#""FLUSHDB""#, false).unwrap();
    assert_eq!(held.confirm.as_deref(), Some("FLUSHDB"));
    assert!(held.reply.is_empty());

    // A key that is not valid UTF-8: listed with its bytes, and opened by them.
    run(&mut session, r#"SET "musql:verify:bin\xff" binary"#);
    let page = redis_client::scan(&mut session, 0, "musql:verify:bin*", || true).unwrap();
    let page = serde_json::to_value(page).unwrap();
    let entry = &page["keys"][0];
    assert_eq!(entry["key"], r"musql:verify:bin\xff");
    let bytes =
        redis_client::key_bytes(entry["key"].as_str().unwrap(), entry["raw"].as_str()).unwrap();
    let view = serde_json::to_value(redis_client::key_view(&mut session, &bytes).unwrap()).unwrap();
    assert_eq!(view["text"], "binary");
    println!("verify_redis:   non-UTF-8 key {} opens", entry["key"]);
    run(&mut session, r#"DEL "musql:verify:bin\xff""#);

    // The DB switcher and the CLI's completion.
    let db = session.db();
    session.select(1).unwrap();
    assert_eq!(session.db(), 1);
    assert!(session.select(100_000).is_err(), "a DB the server lacks");
    assert_eq!(session.db(), 1);
    session.select(db).unwrap();
    let dbs = serde_json::to_value(redis_client::databases(&mut session).unwrap()).unwrap();
    println!(
        "verify_redis:   {} DB(s), DB 0 has {} key(s)",
        dbs.as_array().unwrap().len(),
        dbs[0]["keys"]
    );
    let docs = serde_json::to_value(redis_client::command_docs(&mut session).unwrap()).unwrap();
    let hset = docs
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "HSET")
        .expect("HSET in the command docs");
    println!(
        "verify_redis:   {} command(s); HSET {}",
        docs.as_array().unwrap().len(),
        hset["syntax"]
    );
    // The AI assist's context: the seeded keys show up with their types (key names only).
    let (version, db, sample) = redis_client::ai_context(&mut session, || true).unwrap();
    assert!(
        sample.contains("musql:verify:hash (hash)"),
        "the AI context lists the keys"
    );
    println!(
        "verify_redis:   AI context: Redis {version}, DB {db}, {} key line(s)",
        sample.lines().count()
    );

    let all: Vec<&str> = keys.iter().map(|(key, _)| key.as_str()).collect();
    run(&mut session, &format!("DEL {}", all.join(" ")));
    println!("verify_redis: ok, the seeded keys are deleted again");
}
