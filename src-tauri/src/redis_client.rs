//! Redis connections (#98).
//!
//! A Redis profile keeps its server under the same `mysql` key as every other engine
//! (#132): `username` is the ACL user (empty for the default user), `database` is the DB
//! number, and `ssl_mode` / `tls_ca_cert_path` choose the TLS settings. The SSH tunnel and
//! the credential lookup are shared with MySQL; only the last hop differs.

use crate::{unknown_ssl_mode_error, MySqlConfig, SslMode, CONNECT_TIMEOUT};
use redis::{ConnectionAddr, IntoConnectionInfo, RedisConnectionInfo};

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
}
