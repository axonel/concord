use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::Confidence;
use std::fs;
use sysinfo::System;

pub struct OsInfo {
    pub os: String,
    pub os_family: String,
    pub arch: String,
    pub cpu_count: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub evidence: Vec<Evidence>,
}

/// Scan Linux OS, CPU, and memory details.
pub fn scan_os() -> OsInfo {
    let mut evidence = Vec::new();
    let mut os_name = "Linux".to_string();

    // 1. Inspect /etc/os-release
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("PRETTY_NAME=") {
                os_name = val.trim_matches('"').to_string();
                evidence.push(Evidence::new(
                    EvidenceSource::OsMetadata {
                        key: "PRETTY_NAME".to_string(),
                        value: os_name.clone(),
                    },
                    Confidence::Confirmed,
                    format!("OS identified from /etc/os-release: {}", os_name),
                ));
                break;
            }
        }
    } else {
        evidence.push(Evidence::new(
            EvidenceSource::DirectObservation {
                detail: "Standard /etc/os-release not readable".to_string(),
            },
            Confidence::Low,
            "OS identified as generic Linux",
        ));
    }

    let arch = std::env::consts::ARCH.to_string();
    evidence.push(Evidence::new(
        EvidenceSource::OsMetadata {
            key: "architecture".to_string(),
            value: arch.clone(),
        },
        Confidence::Confirmed,
        format!("System architecture: {}", arch),
    ));

    // 2. Memory & CPU via sysinfo
    let mut sys = System::new_all();
    sys.refresh_memory();
    sys.refresh_cpu();

    let cpu_count = sys.cpus().len();
    let total_memory_bytes = sys.total_memory();
    let available_memory_bytes = sys.available_memory();

    evidence.push(Evidence::new(
        EvidenceSource::OsMetadata {
            key: "cpu_count".to_string(),
            value: cpu_count.to_string(),
        },
        Confidence::Confirmed,
        format!("Detected {} logical CPU cores", cpu_count),
    ));

    evidence.push(Evidence::new(
        EvidenceSource::OsMetadata {
            key: "total_memory_bytes".to_string(),
            value: total_memory_bytes.to_string(),
        },
        Confidence::Confirmed,
        format!(
            "Total memory: {} bytes ({:.1} GB)",
            total_memory_bytes,
            total_memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        ),
    ));

    OsInfo {
        os: os_name,
        os_family: "linux".to_string(),
        arch,
        cpu_count,
        total_memory_bytes,
        available_memory_bytes,
        evidence,
    }
}
