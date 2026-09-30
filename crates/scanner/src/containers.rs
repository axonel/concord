use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use concord_core::evidence::Evidence;
use concord_core::ir::{ContainerObservation, ContainerPortMapping, ContainerStatus};

/// Parse Docker port mapping strings such as "0.0.0.0:6543->5432/tcp, [::]:6543->5432/tcp".
pub fn parse_port_mappings(ports_str: &str) -> Vec<ContainerPortMapping> {
    let mut mappings = Vec::new();
    for part in ports_str.split(',') {
        let trimmed = part.trim();
        // Format: [HOST_IP:]HOST_PORT->CONTAINER_PORT/PROTO
        if let Some((host_part, container_part)) = trimmed.split_once("->") {
            let (container_port_str, proto) = if let Some((p, pr)) = container_part.split_once('/')
            {
                (p, pr)
            } else {
                (container_part, "tcp")
            };

            let container_port = match container_port_str.parse::<u16>() {
                Ok(p) => p,
                Err(_) => continue,
            };

            let (host_ip, host_port_str) = if let Some(idx) = host_part.rfind(':') {
                let ip = &host_part[..idx];
                let port = &host_part[idx + 1..];
                (Some(ip.to_string()), port)
            } else {
                (None, host_part)
            };

            if let Ok(host_port) = host_port_str.parse::<u16>() {
                if !mappings.iter().any(|m: &ContainerPortMapping| {
                    m.host_port == host_port && m.container_port == container_port
                }) {
                    mappings.push(ContainerPortMapping {
                        host_ip,
                        host_port,
                        container_port,
                        protocol: proto.to_string(),
                    });
                }
            }
        }
    }
    mappings
}

/// Parse labels string formatted as comma-separated key=value pairs.
pub fn parse_labels(labels_str: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for item in labels_str.split(',') {
        let trimmed = item.trim();
        if let Some((k, v)) = trimmed.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

/// Parse container status from Docker JSON fields.
pub fn parse_container_status(state: &str, status: &str, health: &str) -> ContainerStatus {
    match state.to_lowercase().as_str() {
        "running" => {
            let healthy = match health.to_lowercase().as_str() {
                "healthy" => Some(true),
                "unhealthy" => Some(false),
                _ => None,
            };
            ContainerStatus::Running { healthy }
        }
        "exited" => {
            // E.g. "Exited (0) 21 hours ago" or "Exited (137) 2 days ago"
            let exit_code = if let Some(start) = status.find('(') {
                if let Some(end) = status[start + 1..].find(')') {
                    status[start + 1..start + 1 + end]
                        .parse::<i32>()
                        .unwrap_or(0)
                } else {
                    0
                }
            } else {
                0
            };
            ContainerStatus::Exited { exit_code }
        }
        "created" => ContainerStatus::Created,
        "paused" => ContainerStatus::Paused,
        "dead" => ContainerStatus::Dead,
        other => ContainerStatus::Unknown(format!("{}: {}", other, status)),
    }
}

/// Scan containers currently on the host machine using `docker ps -a --format json`.
pub fn scan_containers(path_entries: &[PathBuf]) -> Vec<ContainerObservation> {
    let docker_bin = path_entries.iter().find_map(|dir| {
        let p = dir.join("docker");
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    });

    let bin = match docker_bin {
        Some(b) => b,
        None => {
            let default_path = Path::new("/usr/bin/docker");
            if default_path.is_file() {
                default_path.to_path_buf()
            } else {
                return Vec::new();
            }
        }
    };

    let output = match Command::new(&bin)
        .args(["ps", "-a", "--format", "json"])
        .output()
    {
        Ok(out) if out.status.success() => out,
        _ => return Vec::new(),
    };

    let stdout_str = String::from_utf8_lossy(&output.stdout);
    let mut containers = Vec::new();

    for line in stdout_str.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let id = val
                .get("ID")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let names_raw = val.get("Names").and_then(|v| v.as_str()).unwrap_or("");
            let names: Vec<String> = names_raw
                .split(',')
                .map(|s| s.trim().trim_start_matches('/').to_string())
                .filter(|s| !s.is_empty())
                .collect();
            let image = val
                .get("Image")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let state = val.get("State").and_then(|v| v.as_str()).unwrap_or("");
            let status_desc = val.get("Status").and_then(|v| v.as_str()).unwrap_or("");
            let health = val
                .get("HealthStatus")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let labels_raw = val.get("Labels").and_then(|v| v.as_str()).unwrap_or("");
            let ports_raw = val.get("Ports").and_then(|v| v.as_str()).unwrap_or("");

            let status = parse_container_status(state, status_desc, health);
            let labels = parse_labels(labels_raw);
            let ports = parse_port_mappings(ports_raw);

            let compose_project = labels.get("com.docker.compose.project").cloned();
            let compose_service = labels.get("com.docker.compose.service").cloned();

            let primary_name = names.first().cloned().unwrap_or_else(|| id.clone());
            let ev = Evidence::from_executable(
                bin.clone(),
                format!(
                    "Container '{}' ({}, status: {})",
                    primary_name, image, status
                ),
                format!("docker ps -a (container ID: {})", id),
            );

            containers.push(ContainerObservation {
                id,
                names,
                image,
                status,
                ports,
                compose_project,
                compose_service,
                labels,
                evidence: ev,
            });
        }
    }

    containers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_container_status() {
        assert_eq!(
            parse_container_status("running", "Up 2 hours (healthy)", "healthy"),
            ContainerStatus::Running {
                healthy: Some(true)
            }
        );
        assert_eq!(
            parse_container_status("running", "Up 2 hours", "none"),
            ContainerStatus::Running { healthy: None }
        );
        assert_eq!(
            parse_container_status("exited", "Exited (0) 21 hours ago", "none"),
            ContainerStatus::Exited { exit_code: 0 }
        );
        assert_eq!(
            parse_container_status("exited", "Exited (137) 5 minutes ago", "none"),
            ContainerStatus::Exited { exit_code: 137 }
        );
    }

    #[test]
    fn test_parse_port_mappings() {
        let mappings = parse_port_mappings("0.0.0.0:6543->5432/tcp, [::]:6543->5432/tcp");
        assert_eq!(mappings.len(), 1);
        assert_eq!(mappings[0].host_port, 6543);
        assert_eq!(mappings[0].container_port, 5432);
        assert_eq!(mappings[0].protocol, "tcp");
    }

    #[test]
    fn test_parse_labels() {
        let raw = "com.docker.compose.project=heym,com.docker.compose.service=postgres";
        let labels = parse_labels(raw);
        assert_eq!(labels.get("com.docker.compose.project").unwrap(), "heym");
        assert_eq!(
            labels.get("com.docker.compose.service").unwrap(),
            "postgres"
        );
    }
}
