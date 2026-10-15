use bollard::query_parameters::ListContainersOptions;
use bollard::Docker;
use serde::Serialize;
use std::collections::HashMap;

use crate::{Engine, SslMode};

#[derive(Debug, Serialize, Clone)]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub project: Option<String>,
    /// The server (#98): which window opens the container. A `musql.engine` label this
    /// version does not know stays `Unknown`, listed and refused when opened.
    pub engine: Engine,
    pub port: u16,
    pub networks: Vec<String>,
    pub host_port: Option<u16>,
    pub label_user: Option<String>,
    pub label_password: Option<String>,
    pub label_name: Option<String>,
    pub label_ssl_mode: Option<SslMode>,
}

/// Normalises a `musql.ssl-mode` label value. Only DISABLED and REQUIRED are accepted:
/// a Docker connection has no CA path and targets 127.0.0.1, so the verifying modes
/// cannot succeed. Anything else is dropped and the UI default applies.
fn parse_ssl_mode_label(value: &str) -> Option<SslMode> {
    match SslMode::from(value.trim().to_ascii_uppercase()) {
        mode @ (SslMode::Disabled | SslMode::Required) => Some(mode),
        _ => None,
    }
}

const MYSQL_PORT: u16 = 3306;
const REDIS_PORT: u16 = 6379;

/// Which server a container runs, and on which of its ports: `(engine, port)`, or `None`
/// for a container that is neither.
///
/// - `musql.engine` decides the engine, and `musql.port` the port (else the engine's
///   default). Such a container is always listed, an engine this version does not know
///   included (as `Unknown`: the label is not overridden by a guess from the ports).
/// - Without it, the order keeps what was listed before Redis was supported: a container
///   that exposes `musql.port` (else 3306), or carries `musql.enable=true`, is MySQL; only
///   then does an exposed 6379 make it Redis. `musql.port` is the MySQL port here; a Redis
///   container on another port says `musql.engine=redis`.
fn classify(label: impl Fn(&str) -> Option<String>, exposed: &[u16]) -> Option<(Engine, u16)> {
    let port_label: Option<u16> = label("musql.port").and_then(|v| v.trim().parse().ok());
    if let Some(name) = label("musql.engine") {
        let engine = Engine::from(name.trim().to_ascii_lowercase());
        let default_port = match engine {
            Engine::Redis => REDIS_PORT,
            _ => MYSQL_PORT,
        };
        return Some((engine, port_label.unwrap_or(default_port)));
    }
    let mysql_port = port_label.unwrap_or(MYSQL_PORT);
    let enabled = label("musql.enable").is_some_and(|v| v == "true");
    if exposed.contains(&mysql_port) || enabled {
        return Some((Engine::Mysql, mysql_port));
    }
    exposed
        .contains(&REDIS_PORT)
        .then_some((Engine::Redis, REDIS_PORT))
}

/// The running containers that run MySQL (MariaDB) or Redis.
pub async fn discover_containers(docker: &Docker) -> Result<Vec<DockerContainer>, String> {
    let mut filters = HashMap::new();
    filters.insert("status".to_owned(), vec!["running".to_owned()]);

    let options = ListContainersOptions {
        filters: Some(filters),
        ..Default::default()
    };

    let containers = docker
        .list_containers(Some(options))
        .await
        .map_err(|e| format!("Failed to list containers: {e}"))?;

    let mut result = Vec::new();

    for c in containers {
        let labels = c.labels.as_ref();
        // Skip socat tunnel containers created by musql
        let is_tunnel = labels
            .and_then(|l| l.get("musql.tunnel"))
            .map(|v| v == "true")
            .unwrap_or(false);
        if is_tunnel {
            continue;
        }

        let exposed: Vec<u16> = c
            .ports
            .as_ref()
            .map(|ports| ports.iter().map(|p| p.private_port).collect())
            .unwrap_or_default();
        let Some((engine, target_port)) =
            classify(|name| labels.and_then(|l| l.get(name)).cloned(), &exposed)
        else {
            continue;
        };

        let id = match c.id.as_ref() {
            Some(id) => id.clone(),
            None => continue,
        };

        // Detect host-bound port (no tunnel needed if present)
        let host_port = c.ports.as_ref().and_then(|ports| {
            ports
                .iter()
                .find(|p| p.private_port == target_port && p.public_port.is_some())
                .and_then(|p| p.public_port)
        });

        // Build display name: musql.name > "project / service" > container name
        let label_name = labels.and_then(|l| l.get("musql.name")).cloned();
        let project = labels
            .and_then(|l| l.get("com.docker.compose.project"))
            .cloned();
        let service = labels
            .and_then(|l| l.get("com.docker.compose.service"))
            .cloned();

        let name = if let Some(ref ln) = label_name {
            ln.clone()
        } else if let (Some(ref proj), Some(ref svc)) = (&project, &service) {
            format!("{proj} / {svc}")
        } else {
            c.names
                .as_ref()
                .and_then(|n| n.first())
                .map(|n| n.trim_start_matches('/').to_owned())
                .unwrap_or_else(|| id[..12].to_string())
        };

        let image = c.image.clone().unwrap_or_default();

        // Collect networks
        let networks: Vec<String> = c
            .network_settings
            .as_ref()
            .and_then(|ns| ns.networks.as_ref())
            .map(|nets| nets.keys().cloned().collect())
            .unwrap_or_default();

        let label_user = labels.and_then(|l| l.get("musql.user")).cloned();
        let label_password = labels.and_then(|l| l.get("musql.password")).cloned();
        let label_ssl_mode = labels
            .and_then(|l| l.get("musql.ssl-mode"))
            .and_then(|v| parse_ssl_mode_label(v));

        result.push(DockerContainer {
            id,
            name,
            image,
            project,
            engine,
            port: target_port,
            networks,
            host_port,
            label_user,
            label_password,
            label_name,
            label_ssl_mode,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_with(labels: &[(&str, &str)], exposed: &[u16]) -> Option<(String, u16)> {
        classify(
            |name| {
                labels
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            },
            exposed,
        )
        .map(|(engine, port)| (String::from(engine), port))
    }

    fn found(engine: &str, port: u16) -> Option<(String, u16)> {
        Some((engine.to_owned(), port))
    }

    #[test]
    fn containers_are_told_apart_by_label_then_port() {
        assert_eq!(classify_with(&[], &[3306]), found("mysql", 3306));
        assert_eq!(classify_with(&[], &[6379]), found("redis", 6379));
        assert_eq!(classify_with(&[], &[5432]), None);
        // musql.port moves the MySQL port.
        assert_eq!(
            classify_with(&[("musql.port", "13306")], &[13306]),
            found("mysql", 13306)
        );
        // musql.enable lists a container as MySQL, as it did before Redis: even one that
        // exposes neither port, or 6379 as well.
        assert_eq!(
            classify_with(&[("musql.enable", "true")], &[]),
            found("mysql", 3306)
        );
        assert_eq!(
            classify_with(&[("musql.enable", "true")], &[6379]),
            found("mysql", 3306)
        );
        // musql.engine decides, with the engine's default port unless musql.port says.
        assert_eq!(
            classify_with(&[("musql.engine", "Redis")], &[]),
            found("redis", 6379)
        );
        assert_eq!(
            classify_with(
                &[("musql.engine", "redis"), ("musql.port", "6380")],
                &[3306]
            ),
            found("redis", 6380)
        );
        assert_eq!(
            classify_with(&[("musql.engine", "mariadb")], &[6379]),
            found("mariadb", 3306)
        );
        // An engine this version does not know is kept, not guessed from the ports.
        assert_eq!(
            classify_with(&[("musql.engine", "postgres")], &[3306]),
            found("postgres", 3306)
        );
    }

    #[test]
    fn parse_ssl_mode_label_normalises_known_modes() {
        assert_eq!(parse_ssl_mode_label("DISABLED"), Some(SslMode::Disabled));
        assert_eq!(parse_ssl_mode_label(" required "), Some(SslMode::Required));
    }

    #[test]
    fn parse_ssl_mode_label_drops_unknown_values() {
        // The verifying modes cannot work over a Docker connection (no CA path).
        assert_eq!(parse_ssl_mode_label("VERIFY_CA"), None);
        assert_eq!(parse_ssl_mode_label("VERIFY_IDENTITY"), None);
        assert_eq!(parse_ssl_mode_label(""), None);
        assert_eq!(parse_ssl_mode_label("true"), None);
        assert_eq!(parse_ssl_mode_label("PREFERRED"), None);
    }
}
