use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::ir::{PortInfo, PortState};
use concord_core::Confidence;
use std::collections::HashMap;
use std::fs;
use std::net::TcpListener;
use std::path::Path;

/// Parse /proc/net/tcp or /proc/net/tcp6 to find listening TCP ports and their socket inodes.
fn parse_proc_net_tcp(path: &str) -> Vec<(u16, u64)> {
    let mut ports = Vec::new();
    if let Ok(content) = fs::read_to_string(path) {
        for line in content.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            // Need at least local_address (index 1), st (index 3), inode (index 9)
            if fields.len() > 9 {
                let state = fields[3];
                // 0A is TCP_LISTEN in hex
                if state == "0A" {
                    let local_addr = fields[1];
                    if let Some((_ip, port_hex)) = local_addr.split_once(':') {
                        if let Ok(port) = u16::from_str_radix(port_hex, 16) {
                            let inode = fields[9].parse::<u64>().unwrap_or(0);
                            ports.push((port, inode));
                        }
                    }
                }
            }
        }
    }
    ports
}

/// Map socket inodes to (PID, Process Name) by inspecting /proc/[pid]/fd.
fn build_inode_process_map() -> HashMap<u64, (u32, String)> {
    let mut map = HashMap::new();
    let proc_dir = Path::new("/proc");

    if let Ok(entries) = fs::read_dir(proc_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            if let Ok(pid) = file_name.to_string_lossy().parse::<u32>() {
                let fd_dir = entry.path().join("fd");
                if let Ok(fd_entries) = fs::read_dir(&fd_dir) {
                    let mut process_name: Option<String> = None;

                    for fd_entry in fd_entries.flatten() {
                        if let Ok(target) = fs::read_link(fd_entry.path()) {
                            let target_str = target.to_string_lossy();
                            if target_str.starts_with("socket:[") && target_str.ends_with(']') {
                                let inode_str = &target_str[8..target_str.len() - 1];
                                if let Ok(inode) = inode_str.parse::<u64>() {
                                    if process_name.is_none() {
                                        let comm_path = entry.path().join("comm");
                                        process_name = fs::read_to_string(comm_path)
                                            .ok()
                                            .map(|s| s.trim().to_string());
                                    }
                                    let comm = process_name
                                        .clone()
                                        .unwrap_or_else(|| "unknown".to_string());
                                    map.insert(inode, (pid, comm));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    map
}

/// Scan listening ports on Linux using /proc/net/tcp and /proc/net/tcp6.
pub fn scan_listening_ports() -> Vec<PortInfo> {
    let mut raw_ports = parse_proc_net_tcp("/proc/net/tcp");
    raw_ports.extend(parse_proc_net_tcp("/proc/net/tcp6"));

    let inode_map = build_inode_process_map();
    let mut seen_ports = std::collections::HashSet::new();
    let mut port_infos = Vec::new();

    for (port, inode) in raw_ports {
        if !seen_ports.insert(port) {
            continue;
        }

        let (pid, process_name) = if let Some((p, name)) = inode_map.get(&inode) {
            (Some(*p), Some(name.clone()))
        } else {
            (None, None)
        };

        let description = match (&pid, &process_name) {
            (Some(p), Some(name)) => format!(
                "Port {} is occupied by process '{}' (PID {})",
                port, name, p
            ),
            (Some(p), None) => format!("Port {} is occupied by PID {}", port, p),
            _ => format!("Port {} is in TCP_LISTEN state (inode {})", port, inode),
        };

        let evidence = Evidence::new(
            EvidenceSource::NetworkProbe {
                target: format!("0.0.0.0:{}", port),
                outcome: "TCP_LISTEN".to_string(),
            },
            Confidence::Confirmed,
            description,
        );

        port_infos.push(PortInfo {
            port,
            state: PortState::Occupied { pid, process_name },
            evidence,
        });
    }

    port_infos.sort_by_key(|p| p.port);
    port_infos
}

/// Check whether a specific port is free by testing bind or checking known listening ports.
pub fn is_port_available(port: u16, known_occupied: &[PortInfo]) -> bool {
    if known_occupied
        .iter()
        .any(|p| p.port == port && matches!(p.state, PortState::Occupied { .. }))
    {
        return false;
    }
    // Double check with a quick local bind test
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_listening_ports_non_crashing() {
        let ports = scan_listening_ports();
        // Just verify it runs and produces valid port numbers without panic
        for p in ports {
            assert!(p.port > 0);
        }
    }
}
