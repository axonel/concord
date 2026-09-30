use serde::{Deserialize, Serialize};
use concord_constraints::model::{Constraint, ConstraintStatus, EvaluatedConstraint};
use concord_core::evidence::Evidence;
use concord_core::ir::{EnvironmentModel, ToolScope};
use concord_core::Confidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredictionCategory {
    RuntimeIncompatibility,
    PackageManagerMissing,
    ToolMissing,
    PortCollision,
    MissingService,
    InsufficientResources,
    ConfigurationMissing,
    ConfigurationConflict,
    OsArchMismatch,
    ComposeConfigMissing,
    ComposeServiceBlocked,
    ContainerStopped,
    ContainerUnhealthy,
    CompilerMissing,
    LanguagePackageMissing,
    SystemLibraryMissing,
    SystemLibraryIncompatible,
    CapabilityUnsatisfied,
}

/// A structured failure prediction derived from deterministic constraint evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prediction {
    pub title: String,
    pub category: PredictionCategory,
    pub summary: String,
    pub confidence: Confidence,
    pub constraint: Constraint,
    pub affected_components: Vec<String>,
    pub project_evidence: Option<Evidence>,
    pub machine_evidence: Option<Evidence>,
}

/// Deterministically predicts environment and runtime failures before execution.
pub fn predict_failures(
    model: &EnvironmentModel,
    evaluated_constraints: &[EvaluatedConstraint],
) -> Vec<Prediction> {
    let mut predictions = Vec::new();

    for eval in evaluated_constraints {
        if let ConstraintStatus::Violated {
            reason,
            root_cause_hint,
        } = &eval.status
        {
            match &eval.constraint {
                Constraint::RuntimeVersion {
                    runtime,
                    constraint,
                } => {
                    let affected = if model.project.languages.iter().any(|l| l.contains(runtime)) {
                        vec![runtime.clone(), "build".to_string(), "startup".to_string()]
                    } else {
                        vec![runtime.clone(), "runtime".to_string()]
                    };

                    predictions.push(Prediction {
                        title: format!("{} runtime incompatibility predicted", runtime),
                        category: PredictionCategory::RuntimeIncompatibility,
                        summary: format!(
                            "Project expects {} {}, but {}. Application startup or build is predicted to fail.",
                            runtime, constraint, reason
                        ),
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: affected,
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::PackageManagerVersion { name, constraint } => {
                    let constraint_desc = match constraint {
                        Some(c) => format!(" {}", c),
                        None => String::new(),
                    };
                    predictions.push(Prediction {
                        title: format!("{} package manager missing or incompatible", name),
                        category: PredictionCategory::PackageManagerMissing,
                        summary: format!(
                            "Project expects package manager {}{}, but {}. Dependency installation is predicted to fail.",
                            name, constraint_desc, reason
                        ),
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![name.clone(), "dependencies".to_string(), "build".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::ToolAvailable {
                    name,
                    kind,
                    constraint,
                    scope,
                } => {
                    if matches!(scope, ToolScope::Optional | ToolScope::DeclaredButUnused) {
                        continue;
                    }
                    let constraint_desc = match constraint {
                        Some(c) => format!(" {}", c),
                        None => String::new(),
                    };
                    let (confidence, consequence) = match scope {
                        ToolScope::RequiredForProject => (
                            Confidence::High,
                            "Project execution or development is predicted to fail.",
                        ),
                        ToolScope::RequiredForBuild => (
                            Confidence::High,
                            "Build or code generation tasks are predicted to fail.",
                        ),
                        ToolScope::RequiredForTask => (
                            Confidence::Medium,
                            "Specific tasks or development scripts requiring this tool will fail, but core application startup may not be blocked.",
                        ),
                        ToolScope::Optional | ToolScope::DeclaredButUnused => (
                            Confidence::Low,
                            "Optional tool is missing; non-essential features or tasks may be unavailable.",
                        ),
                        ToolScope::Unknown => (
                            Confidence::Medium,
                            "Tool is missing; development tasks depending on it may fail.",
                        ),
                    };

                    predictions.push(Prediction {
                        title: format!("{} tool '{}' missing or incompatible", kind, name),
                        category: PredictionCategory::ToolMissing,
                        summary: format!(
                            "Project declares {} '{}'{} ({:?}), but {}. {}",
                            kind, name, constraint_desc, scope, reason, consequence
                        ),
                        confidence,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![
                            name.clone(),
                            format!("{:?}", kind).to_lowercase(),
                        ],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::CompilerAvailable {
                    language,
                    min_standard,
                    constraint,
                } => {
                    let std_str = min_standard
                        .as_deref()
                        .map(|s| format!(" standard {}", s))
                        .unwrap_or_default();
                    let ver_str = constraint
                        .as_ref()
                        .map(|c| format!(" version {}", c))
                        .unwrap_or_default();
                    predictions.push(Prediction {
                        title: format!("{} compiler missing or incompatible", language),
                        category: PredictionCategory::CompilerMissing,
                        summary: format!(
                            "Project requires compiler for '{}{}{}', but {}. Compilation is predicted to fail.",
                            language, std_str, ver_str, reason
                        ),
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![language.clone(), "compiler".to_string(), "build".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::LanguagePackageAvailable {
                    language,
                    package,
                    constraint,
                    scope,
                } => {
                    if matches!(scope, ToolScope::Optional | ToolScope::DeclaredButUnused) {
                        continue;
                    }
                    let ver_str = constraint
                        .as_ref()
                        .map(|c| format!(" {}", c))
                        .unwrap_or_default();
                    predictions.push(Prediction {
                        title: format!("{} package '{}' missing", language, package),
                        category: PredictionCategory::LanguagePackageMissing,
                        summary: format!(
                            "Project requires {} package '{}{}', but {}. Build or execution is predicted to fail.",
                            language, package, ver_str, reason
                        ),
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![format!("{}:{}", language, package), "dependencies".to_string(), "build".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::SystemLibraryAvailable {
                    name,
                    header,
                    constraint,
                    scope,
                } => {
                    if matches!(scope, ToolScope::Optional | ToolScope::DeclaredButUnused) {
                        continue;
                    }
                    let header_clause = header
                        .as_deref()
                        .map(|h| format!(" (header '{}')", h))
                        .unwrap_or_default();
                    let ver_str = constraint
                        .as_ref()
                        .map(|c| format!(" {}", c))
                        .unwrap_or_default();
                    let is_incompatible = root_cause_hint.contains("version_incompatible")
                        || eval.machine_evidence.is_some()
                        || reason.contains("version");
                    let (title, category) = if is_incompatible {
                        (
                            format!("System library '{}' version incompatible", name),
                            PredictionCategory::SystemLibraryIncompatible,
                        )
                    } else {
                        (
                            format!("System library '{}' missing", name),
                            PredictionCategory::SystemLibraryMissing,
                        )
                    };

                    predictions.push(Prediction {
                        title,
                        category,
                        summary: format!(
                            "Project requires system library '{}{}{}', but {}. Build or link phase is predicted to fail.",
                            name, header_clause, ver_str, reason
                        ),
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![name.clone(), "libraries".to_string(), "build".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::AnyOf {
                    capability,
                    constraints,
                    scope,
                } => {
                    if matches!(scope, ToolScope::Optional | ToolScope::DeclaredButUnused) {
                        continue;
                    }
                    let alternatives_str = constraints
                        .iter()
                        .map(|c| match c {
                            Constraint::SystemLibraryAvailable { name, .. } => name.clone(),
                            Constraint::ToolAvailable { name, .. } => name.clone(),
                            Constraint::CompilerAvailable { language, .. } => language.clone(),
                            Constraint::RuntimeVersion { runtime, .. } => runtime.clone(),
                            Constraint::PackageManagerVersion { name, .. } => name.clone(),
                            _ => format!("{}", c),
                        })
                        .collect::<Vec<_>>()
                        .join(" or ");

                    predictions.push(Prediction {
                        title: format!("Capability '{}' unsatisfied", capability),
                        category: PredictionCategory::CapabilityUnsatisfied,
                        summary: format!(
                            "Project requires capability '{}' (provided by {}), but none of the alternatives are satisfied: {}.",
                            capability, alternatives_str, reason
                        ),
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![
                            capability.clone(),
                            "build".to_string(),
                            "dependencies".to_string(),
                        ],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::PortAvailable { port } => {
                    predictions.push(Prediction {
                        title: format!("Port {} collision predicted", port),
                        category: PredictionCategory::PortCollision,
                        summary: format!(
                            "Project expects port {} to be available, but it is currently occupied. Server startup will fail with EADDRINUSE.",
                            port
                        ),
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![format!("port:{}", port), "networking".to_string(), "server".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::ServiceRunning {
                    service,
                    min_version,
                } => {
                    let version_str = min_version
                        .as_deref()
                        .map(|v| format!(" (version >= {})", v))
                        .unwrap_or_default();
                    predictions.push(Prediction {
                        title: format!("Required service '{}' failure predicted", service),
                        category: PredictionCategory::MissingService,
                        summary: format!(
                            "Project depends on service '{}{}', but it is not running or not installed. Connections will be refused.",
                            service, version_str
                        ),
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![service.clone(), "database/backend".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::MemoryMin { min_bytes } => {
                    predictions.push(Prediction {
                        title: "Resource exhaustion (OOM) predicted".to_string(),
                        category: PredictionCategory::InsufficientResources,
                        summary: format!(
                            "Project requires at least {:.1} GB memory, but machine has less. Build or runtime may be terminated by OOM killer.",
                            *min_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
                        ),
                        confidence: Confidence::Medium,
                        constraint: eval.constraint.clone(),
                        affected_components: vec!["system".to_string(), "build".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }

                Constraint::EnvVarSet { key, required } => {
                    if *required {
                        predictions.push(Prediction {
                            title: format!("Required environment variable '{}' missing", key),
                            category: PredictionCategory::ConfigurationMissing,
                            summary: format!(
                                "Project declares required environment variable '{}', but it is not set in the environment.",
                                key
                            ),
                            confidence: Confidence::High,
                            constraint: eval.constraint.clone(),
                            affected_components: vec!["configuration".to_string(), "startup".to_string()],
                            project_evidence: eval.project_evidence.clone(),
                            machine_evidence: None,
                        });
                    }
                }

                Constraint::OsMatch { expected_os } => {
                    predictions.push(Prediction {
                        title: format!("Operating system mismatch: expected {}", expected_os),
                        category: PredictionCategory::OsArchMismatch,
                        summary: format!(
                            "Project specifies operating system '{}', but host OS is '{}'.",
                            expected_os, model.machine.os
                        ),
                        confidence: Confidence::Confirmed,
                        constraint: eval.constraint.clone(),
                        affected_components: vec!["os".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: None,
                    });
                }

                Constraint::ArchMatch { expected_arch } => {
                    predictions.push(Prediction {
                        title: format!("Architecture mismatch: expected {}", expected_arch),
                        category: PredictionCategory::OsArchMismatch,
                        summary: format!(
                            "Project specifies CPU architecture '{}', but host is '{}'.",
                            expected_arch, model.machine.arch
                        ),
                        confidence: Confidence::Confirmed,
                        constraint: eval.constraint.clone(),
                        affected_components: vec!["architecture".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: None,
                    });
                }

                Constraint::ConflictDetected { target, details } => {
                    predictions.push(Prediction {
                        title: format!("Contradictory {} configuration detected", target),
                        category: PredictionCategory::ConfigurationConflict,
                        summary: format!(
                            "Project configuration contains conflicting {} requirements: {}.",
                            target, details
                        ),
                        confidence: Confidence::Confirmed,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![
                            target.clone(),
                            "configuration".to_string(),
                            "build".to_string(),
                        ],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: None,
                    });
                }

                Constraint::ComposeConfigUnresolved {
                    compose_file,
                    project_name,
                    service_name: _,
                    missing_env_files,
                    unresolved_vars,
                    env_templates: _,
                    directly_affected_services,
                    transitively_blocked_services,
                    bootstrap_suggestions: _,
                } => {
                    let mut details = Vec::new();
                    if !missing_env_files.is_empty() {
                        let missing_str = missing_env_files
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ");
                        details.push(format!("required env file '{}' is missing", missing_str));
                    }
                    if !unresolved_vars.is_empty() {
                        details.push(format!("unresolved vars: {}", unresolved_vars.join(", ")));
                    }

                    let p_name = project_name.as_deref().unwrap_or_else(|| {
                        compose_file
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("compose")
                    });

                    let mut affected_components = Vec::new();
                    for s in directly_affected_services {
                        if !affected_components.contains(s) {
                            affected_components.push(s.clone());
                        }
                    }
                    for s in transitively_blocked_services {
                        if !affected_components.contains(s) {
                            affected_components.push(s.clone());
                        }
                    }
                    if affected_components.is_empty() {
                        affected_components.push("compose".to_string());
                    }
                    affected_components.push("docker".to_string());

                    let summary = format!(
                        "Compose project in '{}' cannot be instantiated: {}. Directly affected: [{}]; Transitively blocked: [{}].",
                        compose_file.display(),
                        details.join("; "),
                        directly_affected_services.join(", "),
                        transitively_blocked_services.join(", ")
                    );

                    predictions.push(Prediction {
                        title: format!(
                            "Compose project '{}' cannot start: configuration unresolved",
                            p_name
                        ),
                        category: PredictionCategory::ComposeConfigMissing,
                        summary,
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components,
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: None,
                    });
                }

                Constraint::ComposeServiceState {
                    compose_file,
                    service_name,
                    container_name,
                    expected_state,
                    actual_state,
                } => {
                    let c_str = container_name
                        .as_deref()
                        .map(|c| format!(" (container '{}')", c))
                        .unwrap_or_default();
                    let (category, title, summary) = if actual_state == "not-created" {
                        (
                            PredictionCategory::ComposeServiceBlocked,
                            format!("Compose service '{}' container not created", service_name),
                            format!(
                                "Service '{}' defined in '{}' has no existing container on host.",
                                service_name,
                                compose_file.display()
                            ),
                        )
                    } else if actual_state.starts_with("exited") {
                        (
                            PredictionCategory::ContainerStopped,
                            format!("Compose service '{}' container stopped", service_name),
                            format!(
                                "Container for service '{}'{} defined in '{}' is stopped ({}).",
                                service_name,
                                c_str,
                                compose_file.display(),
                                actual_state
                            ),
                        )
                    } else {
                        (
                            PredictionCategory::ContainerUnhealthy,
                            format!("Compose service '{}' container unhealthy", service_name),
                            format!(
                                "Container for service '{}'{} defined in '{}' is in state '{}' (expected '{}').",
                                service_name,
                                c_str,
                                compose_file.display(),
                                actual_state,
                                expected_state
                            ),
                        )
                    };

                    let mut affected = vec![
                        service_name.clone(),
                        "compose".to_string(),
                        "docker".to_string(),
                    ];
                    if let Some(ref c_name) = container_name {
                        affected.push(c_name.clone());
                    }

                    predictions.push(Prediction {
                        title,
                        category,
                        summary,
                        confidence: Confidence::High,
                        constraint: eval.constraint.clone(),
                        affected_components: affected,
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }
            }
        }

        if let ConstraintStatus::Unknown { reason } = &eval.status {
            if let Constraint::SystemLibraryAvailable {
                name,
                header,
                constraint,
                scope,
            } = &eval.constraint
            {
                if !matches!(scope, ToolScope::Optional | ToolScope::DeclaredButUnused) {
                    let header_clause = header
                        .as_deref()
                        .map(|h| format!(" (header '{}')", h))
                        .unwrap_or_default();
                    let ver_str = constraint
                        .as_ref()
                        .map(|c| format!(" {}", c))
                        .unwrap_or_default();

                    predictions.push(Prediction {
                        title: format!("System library '{}' version unknown", name),
                        category: PredictionCategory::SystemLibraryIncompatible,
                        summary: format!(
                            "Project requires system library '{}{}{}', but {}. Build or link compatibility cannot be verified.",
                            name, header_clause, ver_str, reason
                        ),
                        confidence: Confidence::Unknown,
                        constraint: eval.constraint.clone(),
                        affected_components: vec![name.clone(), "libraries".to_string(), "build".to_string()],
                        project_evidence: eval.project_evidence.clone(),
                        machine_evidence: eval.machine_evidence.clone(),
                    });
                }
            }
        }
    }

    let mut deduped: Vec<Prediction> = Vec::new();
    for p in predictions {
        let is_compose_missing = p.category == PredictionCategory::ComposeConfigMissing;
        let found_existing = if is_compose_missing {
            deduped.iter_mut().find(|e| {
                if e.category == PredictionCategory::ComposeConfigMissing {
                    match (&e.constraint, &p.constraint) {
                        (
                            Constraint::ComposeConfigUnresolved {
                                compose_file: f1,
                                missing_env_files: m1,
                                ..
                            },
                            Constraint::ComposeConfigUnresolved {
                                compose_file: f2,
                                missing_env_files: m2,
                                ..
                            },
                        ) => f1 == f2 && m1 == m2,
                        _ => false,
                    }
                } else {
                    false
                }
            })
        } else {
            deduped.iter_mut().find(|e| e.constraint == p.constraint)
        };

        if let Some(existing) = found_existing {
            for comp in p.affected_components {
                if !existing.affected_components.contains(&comp) {
                    existing.affected_components.push(comp);
                }
            }
            if is_compose_missing {
                let mut services: Vec<String> = existing
                    .affected_components
                    .iter()
                    .filter(|c| c.as_str() != "compose" && c.as_str() != "docker")
                    .cloned()
                    .collect();
                services.sort();
                if services.len() > 1 {
                    existing.title = format!(
                        "Compose configuration unresolved: {} cannot start",
                        services.join(", ")
                    );
                }
            }
        } else {
            deduped.push(p);
        }
    }

    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use concord_core::ir::{MachineCapability, ProjectManifest};

    #[test]
    fn test_predict_runtime_failure() {
        let manifest = ProjectManifest {
            name: "app".to_string(),
            root_path: PathBuf::from("/test/app"),
            languages: vec!["python".to_string()],
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

        let eval = EvaluatedConstraint {
            constraint: Constraint::RuntimeVersion {
                runtime: "python".to_string(),
                constraint: concord_core::version::VersionConstraint::parse(">= 3.11"),
            },
            status: ConstraintStatus::Violated {
                reason: "Runtime 'python' is not installed".to_string(),
                root_cause_hint: "python.missing".to_string(),
            },
            project_evidence: None,
            machine_evidence: None,
        };

        let predictions = predict_failures(&env_model, &[eval]);
        assert_eq!(predictions.len(), 1);
        assert_eq!(
            predictions[0].category,
            PredictionCategory::RuntimeIncompatibility
        );
        assert_eq!(predictions[0].confidence, Confidence::High);
    }

    #[test]
    fn test_predict_package_manager_failure() {
        let manifest = ProjectManifest {
            name: "app".to_string(),
            root_path: PathBuf::from("/test/app"),
            languages: vec!["javascript".to_string()],
            package_managers: vec!["pnpm".to_string()],
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

        let eval = EvaluatedConstraint {
            constraint: Constraint::PackageManagerVersion {
                name: "pnpm".to_string(),
                constraint: Some(concord_core::version::VersionConstraint::parse("11.24.0")),
            },
            status: ConstraintStatus::Violated {
                reason: "Package manager 'pnpm' is not installed".to_string(),
                root_cause_hint: "pnpm.missing".to_string(),
            },
            project_evidence: None,
            machine_evidence: None,
        };

        let predictions = predict_failures(&env_model, &[eval]);
        assert_eq!(predictions.len(), 1);
        assert_eq!(
            predictions[0].category,
            PredictionCategory::PackageManagerMissing
        );
        assert_eq!(predictions[0].confidence, Confidence::High);
    }

    #[test]
    fn test_predict_task_tool_calibrated_confidence() {
        let manifest = ProjectManifest {
            name: "app".to_string(),
            root_path: PathBuf::from("/test/app"),
            languages: vec!["typescript".to_string()],
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

        let eval = EvaluatedConstraint {
            constraint: Constraint::ToolAvailable {
                name: "terragrunt".to_string(),
                kind: concord_core::ir::ToolKind::DeveloperTool,
                constraint: Some(concord_core::version::VersionConstraint::parse("1.1.1")),
                scope: concord_core::ir::ToolScope::RequiredForTask,
            },
            status: ConstraintStatus::Violated {
                reason: "DeveloperTool 'terragrunt' is not installed".to_string(),
                root_cause_hint: "terragrunt.missing".to_string(),
            },
            project_evidence: None,
            machine_evidence: None,
        };

        let predictions = predict_failures(&env_model, &[eval]);
        assert_eq!(predictions.len(), 1);
        assert_eq!(predictions[0].category, PredictionCategory::ToolMissing);
        // Important: task-level tools must be Medium confidence, NOT High
        assert_eq!(predictions[0].confidence, Confidence::Medium);
        assert!(!predictions[0]
            .summary
            .contains("Application startup or build is predicted to fail"));
    }
}
