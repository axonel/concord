pub mod evaluator;
pub mod model;
pub mod version;

pub use evaluator::{
    evaluate_all, evaluate_constraint, evaluate_project, requirement_to_constraint,
    requirement_to_constraint_with_project,
};
pub use model::{Constraint, ConstraintStatus, EvaluatedConstraint};
pub use version::{
    matches_version_constraint, normalize_semver, parse_version_req, VersionComparator,
    VersionConstraint,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use concord_core::evidence::{Evidence, EvidenceSource};
    use concord_core::ir::{
        MachineCapability, PortInfo, PortState, ProjectRequirement, RequirementKind, Runtime,
        Service, ServiceStatus,
    };
    use concord_core::Confidence;

    fn mock_machine() -> MachineCapability {
        MachineCapability {
            os: "Ubuntu 24.04 LTS".to_string(),
            os_family: "linux".to_string(),
            arch: "x86_64".to_string(),
            cpu_count: 8,
            total_memory_bytes: 16 * 1024 * 1024 * 1024,
            available_memory_bytes: 8 * 1024 * 1024 * 1024,
            runtimes: vec![
                Runtime {
                    name: "python".to_string(),
                    version: "3.10.12".to_string(),
                    executable_path: PathBuf::from("/usr/bin/python3"),
                    evidence: Evidence::from_executable(
                        PathBuf::from("/usr/bin/python3"),
                        "Python 3.10.12",
                        "python3 --version",
                    ),
                },
                Runtime {
                    name: "node".to_string(),
                    version: "20.11.0".to_string(),
                    executable_path: PathBuf::from("/usr/bin/node"),
                    evidence: Evidence::from_executable(
                        PathBuf::from("/usr/bin/node"),
                        "v20.11.0",
                        "node --version",
                    ),
                },
            ],
            services: vec![Service {
                name: "docker".to_string(),
                version: None,
                status: ServiceStatus::Running,
                port: None,
                socket_path: Some(PathBuf::from("/var/run/docker.sock")),
                evidence: Evidence::new(
                    EvidenceSource::DirectObservation {
                        detail: "docker socket".to_string(),
                    },
                    Confidence::Confirmed,
                    "Docker active",
                ),
            }],
            listening_ports: vec![PortInfo {
                port: 3000,
                state: PortState::Occupied {
                    pid: Some(9999),
                    process_name: Some("node".to_string()),
                },
                evidence: Evidence::new(
                    EvidenceSource::ProcessInspection {
                        pid: 9999,
                        name: "node".to_string(),
                        cmdline: None,
                    },
                    Confidence::Confirmed,
                    "Port 3000 in use",
                ),
            }],
            package_managers: vec![concord_core::ir::PackageManagerObservation {
                name: "pnpm".to_string(),
                version: Some("11.24.0".to_string()),
                executable_path: PathBuf::from("/usr/bin/pnpm"),
                evidence: Evidence::from_executable(
                    PathBuf::from("/usr/bin/pnpm"),
                    "11.24.0",
                    "pnpm --version",
                ),
            }],
            tools: vec![concord_core::ir::ToolObservation {
                name: "make".to_string(),
                kind: concord_core::ir::ToolKind::BuildTool,
                version: Some("4.4.1".to_string()),
                executable_path: PathBuf::from("/usr/bin/make"),
                evidence: Evidence::from_executable(
                    PathBuf::from("/usr/bin/make"),
                    "GNU Make 4.4.1",
                    "make --version",
                ),
            }],
            containers: vec![],
            env_vars: HashMap::new(),
            path_entries: vec![PathBuf::from("/usr/bin")],
            evidence: vec![],
        }
    }

    #[test]
    fn test_python_version_violated() {
        let machine = mock_machine();
        let constraint = Constraint::RuntimeVersion {
            runtime: "python".to_string(),
            constraint: VersionConstraint::parse(">= 3.11"),
        };
        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_violated());
        if let ConstraintStatus::Violated {
            reason,
            root_cause_hint,
        } = eval.status
        {
            assert!(reason.contains("3.10.12"));
            assert_eq!(root_cause_hint, "python.version_mismatch");
        } else {
            panic!("Expected violation");
        }
    }

    #[test]
    fn test_node_version_satisfied() {
        let machine = mock_machine();
        let constraint = Constraint::RuntimeVersion {
            runtime: "node".to_string(),
            constraint: VersionConstraint::parse(">= 20.0.0"),
        };
        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_satisfied());
    }

    #[test]
    fn test_package_manager_satisfied_and_violated() {
        let machine = mock_machine();
        let satisfied = Constraint::PackageManagerVersion {
            name: "pnpm".to_string(),
            constraint: Some(VersionConstraint::parse("11.24.0")),
        };
        assert!(evaluate_constraint(&satisfied, &machine, None).is_satisfied());

        let violated = Constraint::PackageManagerVersion {
            name: "pnpm".to_string(),
            constraint: Some(VersionConstraint::parse("12.0.0")),
        };
        assert!(evaluate_constraint(&violated, &machine, None).is_violated());

        let missing = Constraint::PackageManagerVersion {
            name: "yarn".to_string(),
            constraint: None,
        };
        assert!(evaluate_constraint(&missing, &machine, None).is_violated());
    }

    #[test]
    fn test_tool_available_satisfied_and_missing() {
        let machine = mock_machine();
        let build_tool = Constraint::ToolAvailable {
            name: "make".to_string(),
            kind: concord_core::ir::ToolKind::BuildTool,
            constraint: None,
            scope: concord_core::ir::ToolScope::RequiredForBuild,
        };
        assert!(evaluate_constraint(&build_tool, &machine, None).is_satisfied());

        let missing_dev_tool = Constraint::ToolAvailable {
            name: "terragrunt".to_string(),
            kind: concord_core::ir::ToolKind::DeveloperTool,
            constraint: Some(VersionConstraint::parse("1.1.1")),
            scope: concord_core::ir::ToolScope::RequiredForTask,
        };
        let eval = evaluate_constraint(&missing_dev_tool, &machine, None);
        assert!(eval.is_violated());
        if let ConstraintStatus::Violated { reason, .. } = eval.status {
            assert!(reason.contains("terragrunt"));
            assert!(reason.contains("RequiredForTask"));
        }
    }

    #[test]
    fn test_port_collision_detected() {
        let machine = mock_machine();
        let constraint = Constraint::PortAvailable { port: 3000 };
        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_violated());
        assert!(eval.machine_evidence.is_some());
    }

    #[test]
    fn test_free_port_satisfied() {
        let machine = mock_machine();
        let constraint = Constraint::PortAvailable { port: 8080 };
        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_satisfied());
    }

    #[test]
    fn test_missing_service_violated() {
        let machine = mock_machine();
        let constraint = Constraint::ServiceRunning {
            service: "postgresql".to_string(),
            min_version: None,
        };
        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_violated());
    }

    #[test]
    fn test_evaluate_all_from_requirements() {
        let machine = mock_machine();
        let requirements = vec![
            ProjectRequirement::new(
                "python",
                RequirementKind::Runtime {
                    name: "python".to_string(),
                    constraint: VersionConstraint::parse(">= 3.11"),
                },
                Evidence::from_repo_file(PathBuf::from("pyproject.toml"), None, "py >= 3.11"),
            ),
            ProjectRequirement::new(
                "port:3000",
                RequirementKind::Port {
                    port: 3000,
                    service_hint: None,
                },
                Evidence::from_repo_file(PathBuf::from("package.json"), None, "port 3000"),
            ),
        ];

        let results = evaluate_all(&requirements, &machine);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_violated()));
    }
}
