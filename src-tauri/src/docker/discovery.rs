use bollard::query_parameters::ListContainersOptions;
use bollard::Docker;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize, Clone)]
pub struct DockerContainer {
    pub id: String,
    pub name: String,
    pub image: String,
    pub project: Option<String>,
    pub port: u16,
    pub networks: Vec<String>,
    pub host_port: Option<u16>,
    pub label_user: Option<String>,
    pub label_password: Option<String>,
    pub label_name: Option<String>,
}

pub async fn discover_mysql_containers(docker: &Docker) -> Result<Vec<DockerContainer>, String> {
    let mut filters = HashMap::new();
    filters.insert("status".to_string(), vec!["running".to_string()]);

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

        let musql_enable = labels
            .and_then(|l| l.get("musql.enable"))
            .map(|v| v == "true")
            .unwrap_or(false);

        // Check if container exposes port 3306 (or custom musql.port)
        let target_port: u16 = labels
            .and_then(|l| l.get("musql.port"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(3306);

        let has_mysql_port = c
            .ports
            .as_ref()
            .is_some_and(|ports| ports.iter().any(|p| p.private_port == target_port));

        if !has_mysql_port && !musql_enable {
            continue;
        }

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
                .map(|n| n.trim_start_matches('/').to_string())
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

        result.push(DockerContainer {
            id,
            name,
            image,
            project,
            port: target_port,
            networks,
            host_port,
            label_user,
            label_password,
            label_name,
        });
    }

    Ok(result)
}
