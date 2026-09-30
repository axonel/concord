use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::ir::{PortInfo, PortState, Service, ServiceStatus};
use concord_core::Confidence;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Probe Docker service status.
pub fn probe_docker(path_entries: &[PathBuf]) -> Service {
    let standard_socket = Path::new("/var/run/docker.sock");
    let user_socket = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .map(|d| PathBuf::from(d).join("docker.sock"));

    let socket_found = if standard_socket.exists() {
        Some(standard_socket.to_path_buf())
    } else if let Some(ref u) = user_socket {
        if u.exists() {
            Some(u.clone())
        } else {
            None
        }
    } else {
        None
    };

    // Look for docker binary in PATH
    let docker_bin = path_entries.iter().find_map(|dir| {
        let p = dir.join("docker");
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    });

    if let Some(sock) = socket_found {
        // Test if docker daemon is responsive via docker info or ping
        let responsive = if let Some(ref bin) = docker_bin {
            Command::new(bin)
                .arg("info")
                .output()
                .map(|out| out.status.success())
                .unwrap_or(false)
        } else {
            // Socket exists, assume running with high confidence
            true
        };

        let (status, conf, desc) = if responsive {
            (
                ServiceStatus::Running,
                Confidence::Confirmed,
                format!(
                    "Docker daemon is active and responsive at socket {}",
                    sock.display()
                ),
            )
        } else {
            (
                ServiceStatus::Stopped,
                Confidence::High,
                format!(
                    "Docker socket exists at {} but daemon is not responding",
                    sock.display()
                ),
            )
        };

        Service {
            name: "docker".to_string(),
            version: None,
            status,
            port: None,
            socket_path: Some(sock.clone()),
            evidence: Evidence::new(
                EvidenceSource::DirectObservation {
                    detail: format!("Socket at {}", sock.display()),
                },
                conf,
                desc,
            ),
        }
    } else if docker_bin.is_some() {
        Service {
            name: "docker".to_string(),
            version: None,
            status: ServiceStatus::Stopped,
            port: None,
            socket_path: None,
            evidence: Evidence::new(
                EvidenceSource::DirectObservation {
                    detail: "docker binary present in PATH but no active docker.sock found"
                        .to_string(),
                },
                Confidence::High,
                "Docker binary installed but daemon is stopped or socket missing",
            ),
        }
    } else {
        Service {
            name: "docker".to_string(),
            version: None,
            status: ServiceStatus::NotInstalled,
            port: None,
            socket_path: None,
            evidence: Evidence::new(
                EvidenceSource::DirectObservation {
                    detail: "Neither docker binary nor docker.sock found".to_string(),
                },
                Confidence::High,
                "Docker is not installed on this machine",
            ),
        }
    }
}

/// Probe PostgreSQL service status.
pub fn probe_postgresql(path_entries: &[PathBuf], listening_ports: &[PortInfo]) -> Service {
    let pg_port_listening = listening_ports
        .iter()
        .find(|p| p.port == 5432 && matches!(p.state, PortState::Occupied { .. }));

    let unix_sockets = [
        Path::new("/var/run/postgresql/.s.PGSQL.5432"),
        Path::new("/tmp/.s.PGSQL.5432"),
    ];
    let found_socket = unix_sockets
        .iter()
        .find(|p| p.exists())
        .map(|p| p.to_path_buf());

    let psql_bin = path_entries.iter().find_map(|dir| {
        let p = dir.join("psql");
        if p.is_file() {
            Some(p)
        } else {
            None
        }
    });

    let version = if let Some(ref bin) = psql_bin {
        Command::new(bin)
            .arg("--version")
            .output()
            .ok()
            .and_then(|out| {
                let s = String::from_utf8_lossy(&out.stdout);
                // "psql (PostgreSQL) 16.2 ..."
                for word in s.split_whitespace() {
                    let clean = word.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
                    if clean.contains('.') {
                        return Some(clean.to_string());
                    }
                }
                None
            })
    } else {
        None
    };

    let (probe_result, probe_ev) = crate::probes::probe_tcp_port("127.0.0.1", 5432, 100);
    let is_connected = matches!(probe_result, crate::probes::TcpProbeResult::Connected);

    if is_connected || pg_port_listening.is_some() || found_socket.is_some() {
        let desc = if let Some(ref ver) = version {
            format!(
                "PostgreSQL is running (version {}) listening on port 5432 / socket",
                ver
            )
        } else {
            "PostgreSQL is running on port 5432 / unix socket".to_string()
        };

        Service {
            name: "postgresql".to_string(),
            version,
            status: ServiceStatus::Running,
            port: Some(5432),
            socket_path: found_socket,
            evidence: if is_connected {
                probe_ev
            } else {
                Evidence::new(
                    EvidenceSource::NetworkProbe {
                        target: "localhost:5432".to_string(),
                        outcome: "LISTENING".to_string(),
                    },
                    Confidence::Confirmed,
                    desc,
                )
            },
        }
    } else if psql_bin.is_some() {
        Service {
            name: "postgresql".to_string(),
            version,
            status: ServiceStatus::Stopped,
            port: Some(5432),
            socket_path: None,
            evidence: probe_ev,
        }
    } else {
        Service {
            name: "postgresql".to_string(),
            version: None,
            status: ServiceStatus::NotInstalled,
            port: Some(5432),
            socket_path: None,
            evidence: probe_ev,
        }
    }
}

/// Scan all recognized services.
pub fn scan_services(path_entries: &[PathBuf], listening_ports: &[PortInfo]) -> Vec<Service> {
    vec![
        probe_docker(path_entries),
        probe_postgresql(path_entries, listening_ports),
    ]
}
