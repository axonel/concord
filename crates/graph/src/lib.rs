pub mod builder;
pub mod model;

pub use builder::EnvironmentGraph;
pub use model::{CausalTrace, EdgeData, NodeData};

#[cfg(test)]
mod tests {
    use super::*;
    use concord_constraints::model::{Constraint, ConstraintStatus, EvaluatedConstraint};
    use concord_core::evidence::Evidence;
    use concord_core::ir::{
        EnvironmentModel, MachineCapability, ProjectManifest, ProjectRequirement, RequirementKind,
        Runtime,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_graph_build_and_causal_trace() {
        let manifest = ProjectManifest {
            name: "web-app".to_string(),
            root_path: PathBuf::from("/test/web-app"),
            languages: vec!["python".to_string()],
            package_managers: vec!["uv".to_string()],
            requirements: vec![ProjectRequirement::new(
                "python".to_string(),
                RequirementKind::Runtime {
                    name: "python".to_string(),
                    constraint: concord_core::version::VersionConstraint::GreaterEqual(
                        "3.11".to_string(),
                    ),
                },
                Evidence::from_repo_file(
                    PathBuf::from("pyproject.toml"),
                    Some(10),
                    "requires-python >= 3.11",
                ),
            )],
            declared_ports: vec![],
            env_vars: vec![],
            env_var_specs: vec![],
            components: vec![],
            compose_projects: vec![],
            bootstrap_actions: vec![],
            docker_used: false,
            evidence: vec![],
        };

        let machine = MachineCapability {
            os: "Ubuntu 24.04".to_string(),
            os_family: "linux".to_string(),
            arch: "x86_64".to_string(),
            cpu_count: 4,
            total_memory_bytes: 8 * 1024 * 1024 * 1024,
            available_memory_bytes: 4 * 1024 * 1024 * 1024,
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
            listening_ports: vec![],
            env_vars: HashMap::new(),
            path_entries: vec![PathBuf::from("/usr/bin")],
            evidence: vec![],
        };

        let env_model = EnvironmentModel::new(manifest, machine);

        let eval = EvaluatedConstraint {
            constraint: Constraint::RuntimeVersion {
                runtime: "python".to_string(),
                constraint: concord_core::VersionConstraint::GreaterEqual("3.11".to_string()),
            },
            status: ConstraintStatus::Violated {
                reason: "Runtime 'python' version 3.10.12 does not satisfy requirement >= 3.11"
                    .to_string(),
                root_cause_hint: "python.version_mismatch".to_string(),
            },
            project_evidence: Some(Evidence::from_repo_file(
                PathBuf::from("pyproject.toml"),
                Some(10),
                "requires-python >= 3.11",
            )),
            machine_evidence: Some(Evidence::from_executable(
                PathBuf::from("/usr/bin/python3"),
                "Python 3.10.12",
                "python3 --version",
            )),
        };

        let graph = EnvironmentGraph::build(&env_model, &[eval]);
        let violations = graph.find_violations();
        assert_eq!(violations.len(), 1);

        let trace = graph
            .trace_causal_chain(violations[0])
            .expect("causal trace");
        assert!(trace.project_evidence.is_some());
        assert!(trace.machine_state.is_some());
        assert!(trace.machine_state.unwrap().contains("3.10.12"));
    }
}
