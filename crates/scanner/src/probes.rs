use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::Confidence;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpProbeResult {
    Connected,
    ConnectionRefused,
    TimedOut,
    Error(String),
}

/// Perform a controlled, non-destructive dynamic TCP socket connection probe.
pub fn probe_tcp_port(host: &str, port: u16, timeout_ms: u64) -> (TcpProbeResult, Evidence) {
    let target = format!("{}:{}", host, port);
    let timeout = Duration::from_millis(timeout_ms);

    // Resolve socket address (defaults to 127.0.0.1 if host is localhost)
    let addr_str = if host == "localhost" {
        "127.0.0.1"
    } else {
        host
    };
    let sock_addr = format!("{}:{}", addr_str, port).parse::<SocketAddr>();

    let outcome_result = match sock_addr {
        Ok(addr) => match TcpStream::connect_timeout(&addr, timeout) {
            Ok(_stream) => TcpProbeResult::Connected,
            Err(e) => match e.kind() {
                std::io::ErrorKind::ConnectionRefused => TcpProbeResult::ConnectionRefused,
                std::io::ErrorKind::TimedOut => TcpProbeResult::TimedOut,
                _ => TcpProbeResult::Error(e.to_string()),
            },
        },
        Err(e) => TcpProbeResult::Error(format!("Address resolution failed: {}", e)),
    };

    let (outcome_str, confidence, description) = match &outcome_result {
        TcpProbeResult::Connected => (
            "CONNECTED".to_string(),
            Confidence::Confirmed,
            format!(
                "Dynamic TCP probe to {} succeeded (connection accepted)",
                target
            ),
        ),
        TcpProbeResult::ConnectionRefused => (
            "CONNECTION_REFUSED".to_string(),
            Confidence::Confirmed,
            format!("Dynamic TCP probe to {} failed: connection refused", target),
        ),
        TcpProbeResult::TimedOut => (
            "TIMED_OUT".to_string(),
            Confidence::High,
            format!(
                "Dynamic TCP probe to {} timed out after {}ms",
                target, timeout_ms
            ),
        ),
        TcpProbeResult::Error(err) => (
            format!("ERROR: {}", err),
            Confidence::Medium,
            format!("Dynamic TCP probe to {} failed: {}", target, err),
        ),
    };

    let evidence = Evidence::new(
        EvidenceSource::DynamicProbe {
            target,
            probe_type: "tcp_connect".to_string(),
            outcome: outcome_str,
        },
        confidence,
        description,
    );

    (outcome_result, evidence)
}

/// Dynamically probe whether the Docker daemon is responsive.
pub fn probe_docker_responsive(docker_bin: Option<&Path>) -> (bool, Option<Evidence>) {
    if let Some(bin) = docker_bin {
        let output = Command::new(bin)
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                let ver = String::from_utf8_lossy(&out.stdout).trim().to_string();
                let ev = Evidence::new(
                    EvidenceSource::DynamicProbe {
                        target: "docker daemon".to_string(),
                        probe_type: "docker info".to_string(),
                        outcome: format!("SUCCESS (version {})", ver),
                    },
                    Confidence::Confirmed,
                    format!(
                        "Docker daemon is responding via dynamic probe (version {})",
                        ver
                    ),
                );
                (true, Some(ev))
            }
            _ => (false, None),
        }
    } else {
        (false, None)
    }
}
