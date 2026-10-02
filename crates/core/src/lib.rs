pub mod arch;
pub mod confidence;
pub mod error;
pub mod evidence;
pub mod ir;
pub mod version;

pub use arch::{
    evaluate_applicability, match_architecture, Architecture, ArchitectureMatch,
    EnvironmentApplicability,
};
pub use confidence::Confidence;
pub use error::{ConcordError, Result};
pub use evidence::{Evidence, EvidenceSource};
pub use ir::*;
pub use version::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_confidence_ordering() {
        assert!(Confidence::Confirmed > Confidence::High);
        assert!(Confidence::High > Confidence::Medium);
        assert!(Confidence::Medium > Confidence::Low);
        assert!(Confidence::Low > Confidence::Unknown);
    }

    #[test]
    fn test_evidence_serialization() {
        let ev = Evidence::from_repo_file(
            PathBuf::from("package.json"),
            Some(12),
            "engines.node constraint",
        );
        let json = serde_json::to_string(&ev).expect("serialize evidence");
        assert!(json.contains("package.json"));
        assert!(json.contains("HIGH"));

        let deserialized: Evidence = serde_json::from_str(&json).expect("deserialize evidence");
        assert_eq!(deserialized, ev);
    }

    #[test]
    fn test_environment_model_serialization() {
        let manifest = ProjectManifest {
            name: "test-app".to_string(),
            root_path: PathBuf::from("/tmp/test-app"),
            languages: vec!["python".to_string()],
            package_managers: vec!["uv".to_string()],
            requirements: vec![ProjectRequirement {
                name: "python".to_string(),
                kind: RequirementKind::Runtime {
                    name: "python".to_string(),
                    constraint: VersionConstraint::parse(">= 3.11"),
                },
                evidence: Evidence::from_repo_file(
                    PathBuf::from("pyproject.toml"),
                    Some(5),
                    "requires-python >= 3.11",
                ),
                additional_evidence: vec![],
                platform: None,
                arch: None,
            }],
            declared_ports: vec![8000],
            env_vars: vec!["DATABASE_URL".to_string()],
            env_var_specs: vec![],
            components: vec![],
            compose_projects: vec![],
            bootstrap_actions: vec![],
            build_system_generations: vec![],
            docker_used: false,
            evidence: vec![],
        };

        let machine = MachineCapability {
            os: "Ubuntu 24.04".to_string(),
            os_family: "linux".to_string(),
            arch: "x86_64".to_string(),
            cpu_count: 8,
            total_memory_bytes: 16 * 1024 * 1024 * 1024,
            available_memory_bytes: 8 * 1024 * 1024 * 1024,
            runtimes: vec![Runtime {
                name: "python".to_string(),
                version: "3.10.12".to_string(),
                executable_path: PathBuf::from("/usr/bin/python3"),
                evidence: Evidence::from_executable(
                    PathBuf::from("/usr/bin/python3"),
                    "Python 3.10.12",
                    "python3 --version",
                ),
            }],
            package_managers: vec![],
            tools: vec![],
            services: vec![],
            containers: vec![],
            listening_ports: vec![PortInfo {
                port: 8000,
                state: PortState::Occupied {
                    pid: Some(1234),
                    process_name: Some("python".to_string()),
                },
                evidence: Evidence::new(
                    EvidenceSource::ProcessInspection {
                        pid: 1234,
                        name: "python".to_string(),
                        cmdline: Some("python -m http.server 8000".to_string()),
                    },
                    Confidence::Confirmed,
                    "Port 8000 occupied by PID 1234",
                ),
            }],
            env_vars: std::collections::HashMap::new(),
            path_entries: vec![PathBuf::from("/usr/bin")],
            evidence: vec![],
        };

        let env = EnvironmentModel::new(manifest, machine);
        let serialized = serde_json::to_string_pretty(&env).expect("serialize env model");
        assert!(serialized.contains("test-app"));
        assert!(serialized.contains("3.10.12"));
        assert!(serialized.contains("8000"));

        let deserialized: EnvironmentModel =
            serde_json::from_str(&serialized).expect("deserialize env model");
        assert_eq!(deserialized, env);
    }
}
