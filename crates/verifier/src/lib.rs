use concord_constraints::model::{Constraint, EvaluatedConstraint};
use concord_core::evidence::Evidence;
use concord_core::ir::EnvironmentModel;
use serde::{Deserialize, Serialize};

/// An individual verification check result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationCheck {
    pub name: String,
    pub category: String,
    pub passed: bool,
    pub message: String,
    pub evidence: Option<Evidence>,
}

/// Structured report of read-only environment verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationReport {
    pub success: bool,
    pub checks: Vec<VerificationCheck>,
    pub total_checks: usize,
    pub passed_checks: usize,
    pub failed_checks: usize,
}

/// Runs read-only verification across discovered requirements and machine state.
pub fn verify_environment(
    model: &EnvironmentModel,
    evaluated_constraints: &[EvaluatedConstraint],
) -> VerificationReport {
    let mut checks = Vec::new();

    // 1. Project Discovery Check
    let req_count = model.project.requirements.len();
    checks.push(VerificationCheck {
        name: "project_discovery".to_string(),
        category: "project".to_string(),
        passed: true,
        message: format!(
            "Discovered {} requirements and {} declared ports across {:?}",
            req_count,
            model.project.declared_ports.len(),
            model.project.languages
        ),
        evidence: model.project.evidence.first().cloned(),
    });

    // 2. Evaluated Constraints Checks
    for eval in evaluated_constraints {
        let (name, category) = match &eval.constraint {
            Constraint::RuntimeVersion { runtime, .. } => {
                (format!("runtime:{}", runtime), "runtime".to_string())
            }
            Constraint::PackageManagerVersion { name, .. } => (
                format!("package_manager:{}", name),
                "package_manager".to_string(),
            ),
            Constraint::ToolAvailable { name, kind, .. } => {
                (format!("tool:{}", name), kind.to_string())
            }
            Constraint::PortAvailable { port } => (format!("port:{}", port), "network".to_string()),
            Constraint::ServiceRunning { service, .. } => {
                (format!("service:{}", service), "service".to_string())
            }
            Constraint::MemoryMin { .. } => ("memory_capacity".to_string(), "resource".to_string()),
            Constraint::EnvVarSet { key, .. } => {
                (format!("env:{}", key), "configuration".to_string())
            }
            Constraint::OsMatch { .. } => ("os_compatibility".to_string(), "system".to_string()),
            Constraint::ArchMatch { .. } => {
                ("arch_compatibility".to_string(), "system".to_string())
            }
            Constraint::ConflictDetected { target, .. } => {
                (format!("conflict:{}", target), "configuration".to_string())
            }
            Constraint::ComposeConfigUnresolved { service_name, .. } => {
                let s_name = service_name.as_deref().unwrap_or("compose");
                (format!("compose_config:{}", s_name), "compose".to_string())
            }
            Constraint::ComposeServiceState { service_name, .. } => (
                format!("compose_service:{}", service_name),
                "compose".to_string(),
            ),
            Constraint::CompilerAvailable { language, .. } => {
                (format!("compiler:{}", language), "compiler".to_string())
            }
            Constraint::LanguagePackageAvailable {
                language, package, ..
            } => (
                format!("package:{}:{}", language, package),
                "package".to_string(),
            ),
            Constraint::SystemLibraryAvailable { name, .. } => {
                (format!("syslib:{}", name), "system_library".to_string())
            }
            Constraint::AnyOf { capability, .. } => (
                format!("capability:{}", capability),
                "capability".to_string(),
            ),
        };

        let passed = eval.is_satisfied();
        let message = if passed {
            format!("Requirement satisfied: {}", eval.constraint)
        } else if let concord_constraints::model::ConstraintStatus::Violated { reason, .. } =
            &eval.status
        {
            reason.clone()
        } else if let concord_constraints::model::ConstraintStatus::Unknown { reason } =
            &eval.status
        {
            format!("Status unknown: {}", reason)
        } else {
            "Status unknown".to_string()
        };

        checks.push(VerificationCheck {
            name,
            category,
            passed,
            message,
            evidence: eval
                .machine_evidence
                .clone()
                .or_else(|| eval.project_evidence.clone()),
        });
    }

    let total_checks = checks.len();
    let passed_checks = checks.iter().filter(|c| c.passed).count();
    let failed_checks = total_checks - passed_checks;
    let success = failed_checks == 0;

    VerificationReport {
        success,
        checks,
        total_checks,
        passed_checks,
        failed_checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use concord_constraints::model::{Constraint, ConstraintStatus};
    use concord_core::ir::{MachineCapability, ProjectManifest};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn test_verify_environment_pass_and_fail() {
        let manifest = ProjectManifest {
            name: "app".to_string(),
            root_path: PathBuf::from("/test/app"),
            languages: vec!["node".to_string()],
            package_managers: vec![],
            requirements: vec![],
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
            os: "Linux".to_string(),
            os_family: "linux".to_string(),
            arch: "x86_64".to_string(),
            cpu_count: 4,
            total_memory_bytes: 1024,
            available_memory_bytes: 512,
            runtimes: vec![],
            package_managers: vec![],
            tools: vec![],
            services: vec![],
            containers: vec![],
            listening_ports: vec![],
            env_vars: HashMap::new(),
            path_entries: vec![],
            evidence: vec![],
        };

        let env_model = EnvironmentModel::new(manifest, machine);

        let eval_pass = EvaluatedConstraint {
            constraint: Constraint::PortAvailable { port: 8080 },
            status: ConstraintStatus::Satisfied,
            project_evidence: None,
            machine_evidence: None,
        };

        let eval_fail = EvaluatedConstraint {
            constraint: Constraint::RuntimeVersion {
                runtime: "node".to_string(),
                constraint: concord_core::version::VersionConstraint::parse(">= 20"),
            },
            status: ConstraintStatus::Violated {
                reason: "Node not installed".to_string(),
                root_cause_hint: "node.missing".to_string(),
            },
            project_evidence: None,
            machine_evidence: None,
        };

        let report = verify_environment(&env_model, &[eval_pass, eval_fail]);
        assert!(!report.success);
        assert_eq!(report.total_checks, 3); // 1 discovery + 2 constraints
        assert_eq!(report.passed_checks, 2);
        assert_eq!(report.failed_checks, 1);
    }
}
