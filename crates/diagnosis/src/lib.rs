use concord_constraints::model::Constraint;
use concord_core::evidence::Evidence;
use concord_core::ir::ToolKind;
use concord_core::Confidence;
use concord_graph::model::CausalTrace;
use concord_predictor::Prediction;
use serde::{Deserialize, Serialize};

/// Structured root-cause diagnosis explaining why an environment failure will occur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnosis {
    pub problem: String,
    pub root_cause: String,
    pub causal_chain: Vec<String>,
    pub violated_constraint: String,
    pub confidence: Confidence,
    pub affected_components: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub affected_services: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directly_affected_services: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitively_blocked_services: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub configuration_template: Option<std::path::PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bootstrap_suggestions: Vec<String>,
    pub project_evidence: Option<Evidence>,
    pub machine_evidence: Option<Evidence>,
}

/// Generates deterministic root-cause diagnoses from predictions and graph causal traces.
pub fn diagnose_all(predictions: &[Prediction], traces: &[CausalTrace]) -> Vec<Diagnosis> {
    let mut diagnoses: Vec<Diagnosis> = Vec::new();

    for pred in predictions {
        let matching_trace = traces.iter().find(|t| t.constraint == pred.constraint);

        let (root_cause, causal_chain) = match &pred.constraint {
            Constraint::RuntimeVersion {
                runtime,
                constraint,
            } => {
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or("runtime missing or version incompatible");

                let chain = vec![
                    format!("Host machine state: {}", actual_state),
                    format!("Project specification: requires {} {}", runtime, constraint),
                    format!("Violated invariant: {}.version satisfies {}", runtime, constraint),
                    format!("Downstream impact: {} toolchain cannot initialize; build and runtime will fail", runtime),
                ];

                (
                    format!("{}.version satisfies {}", runtime, constraint),
                    chain,
                )
            }

            Constraint::PackageManagerVersion { name, constraint } => {
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or("package manager missing or version incompatible");
                let constraint_desc = match constraint {
                    Some(c) => format!(" satisfying {}", c),
                    None => " installed".to_string(),
                };

                let chain = vec![
                    format!("Host machine state: {}", actual_state),
                    format!(
                        "Project specification: requires package manager {}{}",
                        name, constraint_desc
                    ),
                    format!(
                        "Violated invariant: package_manager.{}{}",
                        name, constraint_desc
                    ),
                    format!(
                        "Downstream impact: dependency installation via {} cannot proceed",
                        name
                    ),
                ];

                (
                    format!("package_manager.{}{}", name, constraint_desc),
                    chain,
                )
            }

            Constraint::ToolAvailable {
                name,
                kind,
                constraint,
                scope,
            } => {
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or("tool missing or version incompatible");
                let constraint_desc = match constraint {
                    Some(c) => format!(" satisfying {}", c),
                    None => " installed".to_string(),
                };

                if matches!(kind, ToolKind::CodeGenerator) {
                    let chain = vec![
                        format!("Host machine state: {}", actual_state),
                        format!(
                            "Project specification: declares code generator {}{} (scope: {:?})",
                            name, constraint_desc, scope
                        ),
                        format!("Violated invariant: codegen.{}{}", name, constraint_desc),
                        format!(
                            "Downstream impact: source generation tasks via {} cannot proceed; downstream compilation will fail due to missing generated source",
                            name
                        ),
                    ];

                    (format!("codegen.{}{}", name, constraint_desc), chain)
                } else {
                    let chain = vec![
                        format!("Host machine state: {}", actual_state),
                        format!(
                            "Project specification: declares {} tool {}{} (scope: {:?})",
                            kind, name, constraint_desc, scope
                        ),
                        format!("Violated invariant: tool.{}{}", name, constraint_desc),
                        format!(
                            "Downstream impact: tasks or builds relying on {} will fail",
                            name
                        ),
                    ];

                    (format!("tool.{}{}", name, constraint_desc), chain)
                }
            }

            Constraint::CompilerAvailable {
                language,
                min_standard,
                constraint,
            } => {
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or("compiler missing or incompatible");
                let std_clause = min_standard
                    .as_deref()
                    .map(|s| format!(" supporting {}", s))
                    .unwrap_or_default();
                let ver_clause = constraint
                    .as_ref()
                    .map(|c| format!(" version {}", c))
                    .unwrap_or_default();

                let chain = vec![
                    format!("Host machine state: {}", actual_state),
                    format!(
                        "Project specification: requires {} compiler{}{}",
                        language, std_clause, ver_clause
                    ),
                    format!(
                        "Violated invariant: compiler.{}.available == true",
                        language
                    ),
                    format!(
                        "Downstream impact: compilation for {} sources cannot proceed",
                        language
                    ),
                ];

                (
                    format!("compiler.{}{}{}", language, std_clause, ver_clause),
                    chain,
                )
            }

            Constraint::LanguagePackageAvailable {
                language,
                package,
                constraint,
                scope,
            } => {
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or("package missing from runtime environment");
                let ver_clause = constraint
                    .as_ref()
                    .map(|c| format!(" {}", c))
                    .unwrap_or_default();

                let chain = vec![
                    format!("Host runtime state: {}", actual_state),
                    format!(
                        "Project specification: requires {} package '{}{}' (scope: {:?})",
                        language, package, ver_clause, scope
                    ),
                    format!(
                        "Violated invariant: {}:{}.installed == true",
                        language, package
                    ),
                    format!(
                        "Downstream impact: build scripts or modules depending on '{}' will fail",
                        package
                    ),
                ];

                (
                    format!("{}:{}{}.installed", language, package, ver_clause),
                    chain,
                )
            }

            Constraint::SystemLibraryAvailable {
                name,
                header,
                constraint,
                scope,
            } => {
                let actual_state = pred
                    .machine_evidence
                    .as_ref()
                    .map(|e| e.description.as_str())
                    .or_else(|| matching_trace.and_then(|t| t.machine_state.as_deref()))
                    .unwrap_or("system library or development header missing");
                let header_clause = header
                    .as_deref()
                    .map(|h| format!(" with header '{}'", h))
                    .unwrap_or_default();
                let ver_clause = constraint
                    .as_ref()
                    .map(|c| format!(" {}", c))
                    .unwrap_or_default();

                let is_version_incompatible = pred.category
                    == concord_predictor::PredictionCategory::SystemLibraryIncompatible
                    && pred.title.contains("incompatible");
                let is_version_unknown = pred.title.contains("version unknown");

                if is_version_incompatible {
                    let chain = vec![
                        format!(
                            "Project specification: requires system library '{}{}{}' (scope: {:?})",
                            name, header_clause, ver_clause, scope
                        ),
                        format!("System library constraint: syslib.{} satisfies{}", name, ver_clause),
                        format!("Host system state: {}", actual_state),
                        format!(
                            "Violated invariant: syslib.{}.version satisfies{}",
                            name, ver_clause
                        ),
                        format!(
                            "Downstream impact: native linking or build configuration for '{}' will fail due to incompatible library version",
                            name
                        ),
                    ];
                    (
                        format!("syslib.{}.version_incompatible{}", name, ver_clause),
                        chain,
                    )
                } else if is_version_unknown {
                    let chain = vec![
                        format!(
                            "Project specification: requires system library '{}{}{}' (scope: {:?})",
                            name, header_clause, ver_clause, scope
                        ),
                        format!("System library constraint: syslib.{} satisfies{}", name, ver_clause),
                        format!("Host system state: {}", actual_state),
                        format!(
                            "Unverified invariant: syslib.{}.version satisfies{}",
                            name, ver_clause
                        ),
                        format!(
                            "Downstream impact: native linking or build configuration for '{}' may fail if installed library version is incompatible",
                            name
                        ),
                    ];
                    (
                        format!("syslib.{}.version_unknown{}", name, ver_clause),
                        chain,
                    )
                } else {
                    let chain = vec![
                        format!("Host system state: {}", actual_state),
                        format!(
                            "Project specification: requires system library '{}{}{}' (scope: {:?})",
                            name, header_clause, ver_clause, scope
                        ),
                        format!("Violated invariant: syslib.{}.installed == true", name),
                        format!(
                            "Downstream impact: native linking or build configuration for '{}' will fail",
                            name
                        ),
                    ];

                    (
                        format!("syslib.{}{}{}.installed", name, header_clause, ver_clause),
                        chain,
                    )
                }
            }

            Constraint::AnyOf {
                capability,
                constraints,
                scope,
            } => {
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or("no provider available on host");
                let alts_desc = constraints
                    .iter()
                    .map(|c| format!("{}", c))
                    .collect::<Vec<_>>()
                    .join(" OR ");

                let chain = vec![
                    format!("Host machine state: {}", actual_state),
                    format!(
                        "Project specification: requires capability '{}' (alternatives: [{}]) (scope: {:?})",
                        capability, alts_desc, scope
                    ),
                    format!(
                        "Violated invariant: capability.{}.satisfied == true (at least one provider must be installed)",
                        capability
                    ),
                    format!(
                        "Downstream impact: feature or build requiring '{}' cannot proceed without an alternative provider",
                        capability
                    ),
                ];

                (format!("capability.{}.unsatisfied", capability), chain)
            }

            Constraint::PortAvailable { port } => {
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or("port occupied in kernel socket table");

                let chain = vec![
                    format!("Host network state: {}", actual_state),
                    format!("Project specification: requires port {} to be available", port),
                    format!("Violated invariant: port.{} must be free", port),
                    format!("Downstream impact: application binding to port {} will encounter EADDRINUSE collision", port),
                ];

                (format!("port:{}.free", port), chain)
            }

            Constraint::ServiceRunning {
                service,
                min_version,
            } => {
                let ver_clause = min_version
                    .as_deref()
                    .map(|v| format!(" >= {}", v))
                    .unwrap_or_default();
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or("service stopped or inactive");

                let chain = vec![
                    format!("Host service state: {}", actual_state),
                    format!(
                        "Project specification: depends on active service {}{}",
                        service, ver_clause
                    ),
                    format!("Violated invariant: service.{}.status == RUNNING", service),
                    format!(
                        "Downstream impact: application connections to {} will be refused",
                        service
                    ),
                ];

                (format!("service.{}.running", service), chain)
            }

            Constraint::MemoryMin { min_bytes } => {
                let chain = vec![
                    "Host resource state: available memory is below required threshold".to_string(),
                    format!("Project specification: requires at least {:.1} GB memory", *min_bytes as f64 / (1024.0 * 1024.0 * 1024.0)),
                    "Violated invariant: machine.memory >= project.min_memory".to_string(),
                    "Downstream impact: high risk of out-of-memory termination during build or heavy execution".to_string(),
                ];

                ("memory.minimum_threshold".to_string(), chain)
            }

            Constraint::EnvVarSet { key, .. } => {
                let chain = vec![
                    format!("Host environment state: variable '{}' is unset", key),
                    format!("Project specification: requires '{}' in environment", key),
                    format!("Violated invariant: env.contains('{}')", key),
                    format!(
                        "Downstream impact: application configuration lookup for '{}' will fail",
                        key
                    ),
                ];

                (format!("env.{}.present", key), chain)
            }

            Constraint::OsMatch { expected_os } => {
                let chain = vec![
                    "Host OS differs from project expectations".to_string(),
                    format!("Project specification: requires OS '{}'", expected_os),
                    format!("Violated invariant: host.os == '{}'", expected_os),
                    "Downstream impact: platform-specific scripts or binaries will fail"
                        .to_string(),
                ];

                (format!("os.match({})", expected_os), chain)
            }

            Constraint::ArchMatch { expected_arch } => {
                let chain = vec![
                    "Host architecture differs from project expectations".to_string(),
                    format!(
                        "Project specification: requires architecture '{}'",
                        expected_arch
                    ),
                    format!("Violated invariant: host.arch == '{}'", expected_arch),
                    "Downstream impact: native compilation or prebuilt binary execution will fail"
                        .to_string(),
                ];

                (format!("arch.match({})", expected_arch), chain)
            }
            Constraint::ConflictDetected { target, details } => {
                let chain = vec![
                    format!("Configuration conflict detected: {}", details),
                    format!("Target: {}", target),
                    "Violated invariant: configuration sources must not specify conflicting requirements".to_string(),
                    "Downstream impact: unpredictable build or runtime failures due to toolchain or version disagreement".to_string(),
                ];

                (format!("conflict.{}", target), chain)
            }

            Constraint::ComposeConfigUnresolved {
                compose_file,
                project_name: _,
                service_name: _,
                missing_env_files,
                unresolved_vars,
                env_templates,
                directly_affected_services,
                transitively_blocked_services,
                bootstrap_suggestions,
            } => {
                let mut chain = Vec::new();
                if !missing_env_files.is_empty() {
                    let missing_str = missing_env_files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    chain.push(format!("Compose file references {}", missing_str));
                    chain.push(format!("{} does not exist", missing_str));
                    for t in env_templates {
                        chain.push(format!(
                            "Configuration template found: {}",
                            t.template_path.display()
                        ));
                    }
                } else {
                    chain.push(format!(
                        "Docker Compose project defined in '{}'",
                        compose_file.display()
                    ));
                }
                for suggestion in bootstrap_suggestions {
                    chain.push(format!(
                        "Deterministic bootstrap action found: {}",
                        suggestion
                    ));
                }
                if !unresolved_vars.is_empty() {
                    chain.push(format!(
                        "Required variables cannot be resolved: {}",
                        unresolved_vars.join(", ")
                    ));
                } else {
                    chain.push("Required variables cannot be resolved".to_string());
                }
                chain.push("Compose project cannot be instantiated".to_string());
                if !directly_affected_services.is_empty() {
                    chain.push(format!(
                        "Directly affected services: {}",
                        directly_affected_services.join(", ")
                    ));
                }
                if !transitively_blocked_services.is_empty() {
                    chain.push(format!(
                        "Transitively blocked services: {}",
                        transitively_blocked_services.join(", ")
                    ));
                }
                let mut all_affected = directly_affected_services.clone();
                for s in transitively_blocked_services {
                    if !all_affected.contains(s) {
                        all_affected.push(s.clone());
                    }
                }
                all_affected.sort();
                if !all_affected.is_empty() {
                    chain.push(format!(
                        "dependent services cannot be created: {}",
                        all_affected.join(", ")
                    ));
                }

                let root_cause = if !missing_env_files.is_empty() {
                    format!("missing.env_file:{}", missing_env_files[0].display())
                } else {
                    "unresolved Compose variables".to_string()
                };
                (root_cause, chain)
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
                let chain = vec![
                    format!(
                        "Project specification: defines Compose service '{}'{} in '{}'",
                        service_name,
                        c_str,
                        compose_file.display()
                    ),
                    format!("Host container state: {}", actual_state),
                    format!(
                        "Violated invariant: compose_service.{}.state == '{}'",
                        service_name, expected_state
                    ),
                    format!(
                        "Downstream impact: network connections to service '{}' will fail",
                        service_name
                    ),
                ];
                (format!("compose.{}.state", service_name), chain)
            }

            Constraint::BuildSystemGenerated {
                input_declaration,
                generator_tool,
                version_constraint,
                generated_artifact,
                downstream_build_system,
                bootstrap_script,
                artifact_present,
            } => {
                let actual_state = matching_trace
                    .and_then(|t| t.machine_state.as_deref())
                    .unwrap_or(if *artifact_present {
                        "artifact present"
                    } else {
                        "artifact missing or generator unavailable"
                    });

                if pred.category
                    == concord_predictor::PredictionCategory::BuildSystemGenerationRequired
                {
                    let script_str = bootstrap_script
                        .as_ref()
                        .map(|s| format!(" via '{}'", s.display()))
                        .unwrap_or_default();
                    let chain = vec![
                        format!(
                            "Host machine state: generator '{}' is available; artifact '{}' is not present in repository",
                            generator_tool,
                            generated_artifact.display()
                        ),
                        format!(
                            "Project specification: input declaration '{}' produces '{}' using '{}'{}",
                            input_declaration.display(),
                            generated_artifact.display(),
                            generator_tool,
                            script_str
                        ),
                        format!(
                            "Violated invariant: build_system.{}.generated",
                            downstream_build_system
                        ),
                        format!(
                            "Downstream impact: {} build system configuration is unavailable; run bootstrap step before building",
                            downstream_build_system
                        ),
                    ];
                    (
                        format!("build_system.{}.generated", downstream_build_system),
                        chain,
                    )
                } else if pred.category
                    == concord_predictor::PredictionCategory::CodeGeneratorIncompatible
                {
                    let ver_clause = version_constraint
                        .as_ref()
                        .map(|c| format!(" satisfying {}", c))
                        .unwrap_or_default();
                    let chain = vec![
                        format!("Host machine state: {}", actual_state),
                        format!(
                            "Project specification: requires generator '{}{}' to produce '{}'",
                            generator_tool,
                            ver_clause,
                            generated_artifact.display()
                        ),
                        format!(
                            "Violated invariant: build_system.generator.{}{}",
                            generator_tool, ver_clause
                        ),
                        format!(
                            "Downstream impact: build system artifact '{}' cannot be generated; downstream {} build will fail",
                            generated_artifact.display(),
                            downstream_build_system
                        ),
                    ];
                    (
                        format!("build_system.generator.{}{}", generator_tool, ver_clause),
                        chain,
                    )
                } else {
                    let chain = vec![
                        format!("Host machine state: {}", actual_state),
                        format!(
                            "Project specification: input declaration '{}' requires generator '{}' to produce '{}'",
                            input_declaration.display(),
                            generator_tool,
                            generated_artifact.display()
                        ),
                        format!(
                            "Violated invariant: build_system.generator.{} installed",
                            generator_tool
                        ),
                        format!(
                            "Downstream impact: build system artifact '{}' cannot be generated; downstream {} build will fail",
                            generated_artifact.display(),
                            downstream_build_system
                        ),
                    ];
                    (
                        format!("build_system.generator.{} installed", generator_tool),
                        chain,
                    )
                }
            }
        };

        let final_root_cause = matching_trace
            .and_then(|t| t.root_cause.clone())
            .unwrap_or(root_cause);

        let mut final_causal_chain = matching_trace
            .filter(|t| !t.causal_steps.is_empty())
            .map(|t| t.causal_steps.clone())
            .unwrap_or(causal_chain);

        let affected_components = if !pred.affected_components.is_empty() {
            pred.affected_components.clone()
        } else if let Some(trace) = matching_trace {
            trace.affected_components.clone()
        } else {
            Vec::new()
        };

        let mut affected_services = Vec::new();
        let mut directly_affected_services = Vec::new();
        let mut transitively_blocked_services = Vec::new();
        let mut bootstrap_suggestions = Vec::new();
        let mut configuration_template = None;

        if let Constraint::ComposeConfigUnresolved {
            service_name,
            env_templates,
            directly_affected_services: direct,
            transitively_blocked_services: transitive,
            bootstrap_suggestions: bootstrap,
            ..
        } = &pred.constraint
        {
            if let Some(s) = service_name {
                affected_services.push(s.clone());
            }
            for s in direct {
                if !directly_affected_services.contains(s) {
                    directly_affected_services.push(s.clone());
                }
                if !affected_services.contains(s) {
                    affected_services.push(s.clone());
                }
            }
            for s in transitive {
                if !transitively_blocked_services.contains(s) {
                    transitively_blocked_services.push(s.clone());
                }
                if !affected_services.contains(s) {
                    affected_services.push(s.clone());
                }
            }
            for b in bootstrap {
                if !bootstrap_suggestions.contains(b) {
                    bootstrap_suggestions.push(b.clone());
                }
            }
            for comp in &pred.affected_components {
                if comp != "compose" && comp != "docker" && !affected_services.contains(comp) {
                    affected_services.push(comp.clone());
                }
            }
            affected_services.sort();
            directly_affected_services.sort();
            transitively_blocked_services.sort();
            if let Some(t) = env_templates.first() {
                configuration_template = Some(t.template_path.clone());
            }
        }

        if let Constraint::BuildSystemGenerated {
            input_declaration,
            generator_tool,
            generated_artifact,
            bootstrap_script,
            ..
        } = &pred.constraint
        {
            if let Some(ref s) = bootstrap_script {
                bootstrap_suggestions.push(format!(
                    "Run '{}' to generate '{}' from '{}'",
                    s.display(),
                    generated_artifact.display(),
                    input_declaration.display()
                ));
            } else {
                bootstrap_suggestions.push(format!(
                    "Run '{}' to generate '{}' from '{}'",
                    generator_tool,
                    generated_artifact.display(),
                    input_declaration.display()
                ));
            }
        }

        let is_compose_unresolved = pred.category
            == concord_predictor::PredictionCategory::ComposeConfigMissing
            || matches!(&pred.constraint, Constraint::ComposeConfigUnresolved { .. });

        let problem_title = if is_compose_unresolved {
            "Compose configuration unresolved".to_string()
        } else {
            pred.title.clone()
        };

        if is_compose_unresolved {
            if let Some(existing) = diagnoses
                .iter_mut()
                .find(|d| d.root_cause == final_root_cause)
            {
                for s in affected_services {
                    if !existing.affected_services.contains(&s) {
                        existing.affected_services.push(s);
                    }
                }
                for s in directly_affected_services {
                    if !existing.directly_affected_services.contains(&s) {
                        existing.directly_affected_services.push(s);
                    }
                }
                for s in transitively_blocked_services {
                    if !existing.transitively_blocked_services.contains(&s) {
                        existing.transitively_blocked_services.push(s);
                    }
                }
                for b in bootstrap_suggestions {
                    if !existing.bootstrap_suggestions.contains(&b) {
                        existing.bootstrap_suggestions.push(b);
                    }
                }
                existing.affected_services.sort();
                existing.directly_affected_services.sort();
                existing.transitively_blocked_services.sort();
                for c in &affected_components {
                    if !existing.affected_components.contains(c) {
                        existing.affected_components.push(c.clone());
                    }
                }
                if existing.configuration_template.is_none() {
                    existing.configuration_template = configuration_template;
                }
                existing.problem = "Compose configuration unresolved".to_string();

                continue;
            }
        }

        if let Some(ref tpl) = configuration_template {
            let tpl_step = format!("Configuration template found: {}", tpl.display());
            if !final_causal_chain
                .iter()
                .any(|s| s.contains("Configuration template found"))
            {
                if let Some(pos) = final_causal_chain
                    .iter()
                    .position(|s| s.ends_with("does not exist"))
                {
                    final_causal_chain.insert(pos + 1, tpl_step);
                }
            }
        }

        diagnoses.push(Diagnosis {
            problem: problem_title,
            root_cause: final_root_cause,
            causal_chain: final_causal_chain,
            violated_constraint: pred.constraint.to_string(),
            confidence: pred.confidence,
            affected_components,
            affected_services,
            directly_affected_services,
            transitively_blocked_services,
            configuration_template,
            bootstrap_suggestions,
            project_evidence: pred.project_evidence.clone(),
            machine_evidence: pred.machine_evidence.clone(),
        });
    }

    diagnoses
}

#[cfg(test)]
mod tests {
    use super::*;
    use concord_predictor::PredictionCategory;

    #[test]
    fn test_diagnosis_generation() {
        let pred = Prediction {
            title: "python runtime incompatibility predicted".to_string(),
            category: PredictionCategory::RuntimeIncompatibility,
            summary: "Python 3.10 is installed, >= 3.11 required".to_string(),
            confidence: Confidence::High,
            constraint: Constraint::RuntimeVersion {
                runtime: "python".to_string(),
                constraint: concord_core::version::VersionConstraint::parse(">= 3.11"),
            },
            affected_components: vec!["python".to_string(), "backend".to_string()],
            project_evidence: None,
            machine_evidence: None,
        };

        let diagnoses = diagnose_all(&[pred], &[]);
        assert_eq!(diagnoses.len(), 1);
        let diag = &diagnoses[0];
        assert_eq!(diag.confidence, Confidence::High);
        assert!(diag.root_cause.contains("python.version"));
        assert_eq!(diag.causal_chain.len(), 4);
    }

    #[test]
    fn test_diagnosis_distinguishes_missing_vs_incompatible_version() {
        use concord_core::ir::ToolScope;
        use concord_core::version::VersionConstraint;

        // 1. Missing library
        let missing_pred = Prediction {
            title: "System library 'libevent' missing".to_string(),
            category: PredictionCategory::SystemLibraryMissing,
            summary: "Project requires system library 'libevent >= 2.0', but missing".to_string(),
            confidence: Confidence::High,
            constraint: Constraint::SystemLibraryAvailable {
                name: "libevent".to_string(),
                header: None,
                constraint: Some(VersionConstraint::parse(">= 2.0")),
                scope: ToolScope::RequiredForBuild,
            },
            affected_components: vec!["libevent".to_string()],
            project_evidence: None,
            machine_evidence: None,
        };

        // 2. Version incompatible library
        let incomp_pred = Prediction {
            title: "System library 'libevent' version incompatible".to_string(),
            category: PredictionCategory::SystemLibraryIncompatible,
            summary: "Project requires system library 'libevent >= 2.0', but 1.4 is installed"
                .to_string(),
            confidence: Confidence::High,
            constraint: Constraint::SystemLibraryAvailable {
                name: "libevent".to_string(),
                header: None,
                constraint: Some(VersionConstraint::parse(">= 2.0")),
                scope: ToolScope::RequiredForBuild,
            },
            affected_components: vec!["libevent".to_string()],
            project_evidence: None,
            machine_evidence: Some(Evidence::new(
                concord_core::evidence::EvidenceSource::DynamicProbe {
                    target: "pkg-config libevent".to_string(),
                    probe_type: "pkg-config".to_string(),
                    outcome: "version: 1.4.1".to_string(),
                },
                Confidence::Confirmed,
                "pkg-config metadata exists for 'libevent' (version: 1.4.1)".to_string(),
            )),
        };

        // 3. Version unknown library
        let unknown_pred = Prediction {
            title: "System library 'libevent' version unknown".to_string(),
            category: PredictionCategory::SystemLibraryIncompatible,
            summary: "Project requires system library 'libevent >= 2.0', but version unknown"
                .to_string(),
            confidence: Confidence::Unknown,
            constraint: Constraint::SystemLibraryAvailable {
                name: "libevent".to_string(),
                header: None,
                constraint: Some(VersionConstraint::parse(">= 2.0")),
                scope: ToolScope::RequiredForBuild,
            },
            affected_components: vec!["libevent".to_string()],
            project_evidence: None,
            machine_evidence: Some(Evidence::new(
                concord_core::evidence::EvidenceSource::DirectObservation {
                    detail: "library binary discoverable by linker at '/usr/lib/libevent.so'"
                        .to_string(),
                },
                Confidence::High,
                "library binary discoverable by linker at '/usr/lib/libevent.so'".to_string(),
            )),
        };

        let diags = diagnose_all(&[missing_pred, incomp_pred, unknown_pred], &[]);
        assert_eq!(diags.len(), 3);

        // Missing diagnosis
        assert_eq!(diags[0].problem, "System library 'libevent' missing");
        assert!(diags[0].root_cause.contains("syslib.libevent"));
        assert!(diags[0].root_cause.contains("installed"));
        assert!(diags[0]
            .causal_chain
            .iter()
            .any(|s| s.contains("Violated invariant: syslib.libevent.installed == true")));

        // Incompatible diagnosis
        assert_eq!(
            diags[1].problem,
            "System library 'libevent' version incompatible"
        );
        assert!(diags[1]
            .root_cause
            .contains("syslib.libevent.version_incompatible"));
        assert!(diags[1]
            .causal_chain
            .iter()
            .any(|s| s.contains("Violated invariant: syslib.libevent.version satisfies")));

        // Unknown version diagnosis
        assert_eq!(
            diags[2].problem,
            "System library 'libevent' version unknown"
        );
        assert!(diags[2]
            .root_cause
            .contains("syslib.libevent.version_unknown"));
        assert!(diags[2]
            .causal_chain
            .iter()
            .any(|s| s.contains("Unverified invariant: syslib.libevent.version satisfies")));
    }

    #[test]
    fn test_diagnosis_code_generator() {
        let pred = Prediction {
            title: "Code generator 'bison' missing".to_string(),
            category: concord_predictor::PredictionCategory::CodeGeneratorMissing,
            summary: "Project declares code generator 'bison'".to_string(),
            confidence: Confidence::High,
            constraint: Constraint::ToolAvailable {
                name: "bison".to_string(),
                kind: ToolKind::CodeGenerator,
                constraint: None,
                scope: concord_core::ir::ToolScope::RequiredForBuild,
            },
            affected_components: vec!["bison".to_string(), "codegen".to_string()],
            project_evidence: None,
            machine_evidence: None,
        };

        let diags = diagnose_all(&[pred], &[]);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].problem, "Code generator 'bison' missing");
        assert_eq!(diags[0].root_cause, "codegen.bison installed");
        assert!(diags[0]
            .causal_chain
            .iter()
            .any(|s| s.contains("Violated invariant: codegen.bison installed")));
        assert!(diags[0]
            .causal_chain
            .iter()
            .any(|s| s.contains("source generation tasks via bison cannot proceed")));
    }
}
