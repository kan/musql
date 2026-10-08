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
        ssl_mode: env_or("MUSQL_VERIFY_MYSQL_SSL_MODE", "DISABLED"),
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
        "verify_mysql: {}@{}:{} (ssl: {}){}",
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
    let pool = Pool::new(build_opts(&mysql, &host, port))
        .unwrap_or_else(|e| panic!("failed to build pool: {e}"));
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
    let wants_tls = mysql.ssl_mode != "DISABLED";
    assert_eq!(
        !cipher.is_empty(),
        wants_tls,
        "ssl_mode is {} but the session is {}",
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
        docker::discovery::discover_mysql_containers(&docker).await
    })
    .unwrap_or_else(|e| panic!("Docker failed: {e}"));
    println!(
        "verify_docker: ok, API reachable; {} MySQL container(s)",
        containers.len()
    );
    for c in &containers {
        println!(
            "verify_docker:   {} ({}) port {} -> host {}",
            c.name,
            c.image,
            c.port,
            c.host_port.map_or_else(
                || "(not published, needs a tunnel)".to_owned(),
                |p| p.to_string()
            )
        );
    }
}
