use bollard::models::{ContainerCreateBody, HostConfig, PortBinding};
use bollard::query_parameters::{
    CreateContainerOptions, CreateImageOptions, ListContainersOptions, StopContainerOptions,
};
use bollard::Docker;
use futures_util::TryStreamExt;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

const SOCAT_IMAGE: &str = "alpine/socat:latest";

#[derive(Debug, Serialize, Clone)]
pub struct TunnelInfo {
    pub container_id: String,
    pub local_host: String,
    pub local_port: u16,
}

static DOCKER_TUNNELS: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub async fn ensure_socat_image(docker: &Docker) -> Result<(), String> {
    // Check if image already exists
    if docker.inspect_image(SOCAT_IMAGE).await.is_ok() {
        return Ok(());
    }

    // Pull the image
    let options = CreateImageOptions {
        from_image: Some(SOCAT_IMAGE.to_string()),
        ..Default::default()
    };

    docker
        .create_image(Some(options), None, None)
        .try_collect::<Vec<_>>()
        .await
        .map_err(|e| format!("Failed to pull socat image: {e}"))?;

    Ok(())
}

pub async fn create_tunnel(
    docker: &Docker,
    target_id: &str,
    target_port: u16,
) -> Result<TunnelInfo, String> {
    // Inspect target container to get IP and network
    let info = docker
        .inspect_container(target_id, None)
        .await
        .map_err(|e| format!("Failed to inspect container: {e}"))?;

    let networks = info
        .network_settings
        .as_ref()
        .and_then(|ns| ns.networks.as_ref())
        .ok_or("Target container has no network settings")?;

    // Prefer non-bridge network (compose creates custom networks)
    let (net_name, net_info) = networks
        .iter()
        .find(|(name, _)| name.as_str() != "bridge")
        .or_else(|| networks.iter().next())
        .ok_or("Target container is not attached to any network")?;

    let target_ip = net_info
        .ip_address
        .as_ref()
        .filter(|ip| !ip.is_empty())
        .ok_or("Target container has no IP address")?;

    // Find available port on host
    let local_port = find_available_port()?;

    // Generate tunnel container name
    let tunnel_name = generate_tunnel_name();

    // Build socat command: forward from container's 0.0.0.0:target_port to target_ip:target_port
    let socat_cmd =
        format!("TCP-LISTEN:{target_port},fork,reuseaddr TCP-CONNECT:{target_ip}:{target_port}");

    let mut labels = HashMap::new();
    labels.insert("musql.tunnel".to_string(), "true".to_string());

    let port_binding = PortBinding {
        host_ip: Some("127.0.0.1".to_string()),
        host_port: Some(local_port.to_string()),
    };

    let mut port_bindings = HashMap::new();
    port_bindings.insert(format!("{target_port}/tcp"), Some(vec![port_binding]));

    let host_config = HostConfig {
        auto_remove: Some(true),
        network_mode: Some(net_name.clone()),
        port_bindings: Some(port_bindings),
        ..Default::default()
    };

    let config = ContainerCreateBody {
        image: Some(SOCAT_IMAGE.to_string()),
        cmd: Some(socat_cmd.split_whitespace().map(String::from).collect()),
        labels: Some(labels),
        exposed_ports: Some(vec![format!("{target_port}/tcp")]),
        host_config: Some(host_config),
        ..Default::default()
    };

    let options = CreateContainerOptions {
        name: Some(tunnel_name),
        ..Default::default()
    };

    let created = docker
        .create_container(Some(options), config)
        .await
        .map_err(|e| format!("Failed to create tunnel container: {e}"))?;

    docker
        .start_container(&created.id, None)
        .await
        .map_err(|e| format!("Failed to start tunnel container: {e}"))?;

    // Wait briefly for socat to start
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // Register tunnel
    if let Ok(mut tunnels) = DOCKER_TUNNELS.lock() {
        tunnels.insert(target_id.to_string(), created.id.clone());
    }

    Ok(TunnelInfo {
        container_id: created.id,
        local_host: "127.0.0.1".to_string(),
        local_port,
    })
}

pub async fn stop_tunnel(docker: &Docker, tunnel_container_id: &str) -> Result<(), String> {
    let _ = docker
        .stop_container(
            tunnel_container_id,
            Some(StopContainerOptions {
                t: Some(2),
                ..Default::default()
            }),
        )
        .await;

    // Remove from registry
    if let Ok(mut tunnels) = DOCKER_TUNNELS.lock() {
        tunnels.retain(|_, v| v != tunnel_container_id);
    }

    Ok(())
}

pub async fn cleanup_all_tunnels(docker: &Docker) -> Result<(), String> {
    let mut filters = HashMap::new();
    filters.insert("label".to_string(), vec!["musql.tunnel=true".to_string()]);

    let options = ListContainersOptions {
        filters: Some(filters),
        ..Default::default()
    };

    let containers = docker
        .list_containers(Some(options))
        .await
        .map_err(|e| format!("Failed to list tunnel containers: {e}"))?;

    for c in containers {
        if let Some(id) = c.id {
            let _ = docker
                .stop_container(
                    &id,
                    Some(StopContainerOptions {
                        t: Some(2),
                        ..Default::default()
                    }),
                )
                .await;
        }
    }

    // Clear registry
    if let Ok(mut tunnels) = DOCKER_TUNNELS.lock() {
        tunnels.clear();
    }

    Ok(())
}

fn find_available_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to find available port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local addr: {e}"))?
        .port();
    Ok(port)
}

fn generate_tunnel_name() -> String {
    use std::time::SystemTime;
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("musql-tunnel-{:x}", ts)
}
