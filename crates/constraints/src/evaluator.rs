use crate::model::{Constraint, ConstraintStatus, EvaluatedConstraint};
use crate::version::matches_version_constraint;
use std::path::PathBuf;
use std::process::Command;
use concord_core::evidence::{Evidence, EvidenceSource};
use concord_core::ir::{
    MachineCapability, ProjectRequirement, RequirementKind, ServiceStatus, ToolKind, ToolScope,
};
use concord_core::{Confidence, VersionConstraint};

/// Convert a ProjectRequirement into a Constraint.
pub fn requirement_to_constraint(req: &ProjectRequirement) -> Option<Constraint> {
    match &req.kind {
        RequirementKind::Runtime { name, constraint } => Some(Constraint::RuntimeVersion {
            runtime: name.clone(),
            constraint: constraint.clone(),
        }),
        RequirementKind::PackageManager { name, constraint } => {
            Some(Constraint::PackageManagerVersion {
                name: name.clone(),
                constraint: constraint.clone(),
            })
        }
        RequirementKind::DeveloperTool {
            name,
            constraint,
            scope,
        } => Some(Constraint::ToolAvailable {
            name: name.clone(),
            kind: ToolKind::DeveloperTool,
            constraint: constraint.clone(),
            scope: *scope,
        }),
        RequirementKind::BuildTool {
            name,
            constraint,
            scope,
        } => Some(Constraint::ToolAvailable {
            name: name.clone(),
            kind: ToolKind::BuildTool,
            constraint: constraint.clone(),
            scope: *scope,
        }),
        RequirementKind::CodeGenerator {
            name,
            constraint,
            scope,
        } => Some(Constraint::ToolAvailable {
            name: name.clone(),
            kind: ToolKind::CodeGenerator,
            constraint: constraint.clone(),
            scope: *scope,
        }),
        RequirementKind::Port { port, .. } => Some(Constraint::PortAvailable { port: *port }),
        RequirementKind::Service { name, min_version } => Some(Constraint::ServiceRunning {
            service: name.clone(),
            min_version: min_version.clone(),
        }),
        RequirementKind::Os { name } => Some(Constraint::OsMatch {
            expected_os: name.clone(),
        }),
        RequirementKind::Arch { name } => Some(Constraint::ArchMatch {
            expected_arch: name.clone(),
        }),
        RequirementKind::Memory { min_bytes } => Some(Constraint::MemoryMin {
            min_bytes: *min_bytes,
        }),
        RequirementKind::EnvVar { name, required, .. } => Some(Constraint::EnvVarSet {
            key: name.clone(),
            required: *required,
        }),
        RequirementKind::Conflict {
            target, details, ..
        } => Some(Constraint::ConflictDetected {
            target: target.clone(),
            details: details.clone(),
        }),
        RequirementKind::Compiler {
            language,
            min_standard,
            constraint,
        } => Some(Constraint::CompilerAvailable {
            language: language.clone(),
            min_standard: min_standard.clone(),
            constraint: constraint.clone(),
        }),
        RequirementKind::LanguagePackage {
            language,
            package,
            constraint,
            scope,
        } => Some(Constraint::LanguagePackageAvailable {
            language: language.clone(),
            package: package.clone(),
            constraint: constraint.clone(),
            scope: *scope,
        }),
        RequirementKind::SystemLibrary {
            name,
            header,
            constraint,
            scope,
        } => Some(Constraint::SystemLibraryAvailable {
            name: name.clone(),
            header: header.clone(),
            constraint: constraint.clone(),
            scope: *scope,
        }),
        RequirementKind::AnyOf {
            capability,
            alternatives,
            scope,
        } => {
            let sub_constraints: Vec<Constraint> = alternatives
                .iter()
                .filter_map(requirement_to_constraint)
                .collect();
            if sub_constraints.is_empty() {
                None
            } else {
                Some(Constraint::AnyOf {
                    capability: capability.clone(),
                    constraints: sub_constraints,
                    scope: *scope,
                })
            }
        }
    }
}

fn compiler_supports_standard(
    compiler_name: &str,
    lang: &str,
    standard: &str,
    tool_version: Option<&str>,
) -> (bool, Option<String>) {
    let std_lower = standard.to_lowercase();
    let lang_lower = lang.to_lowercase();

    // 1. Direct active capability probe (safe, read-only preprocessor check to /dev/null)
    let std_arg = if std_lower.starts_with("c++")
        || std_lower.starts_with("gnu++")
        || std_lower.starts_with("c")
        || std_lower.starts_with("f")
    {
        format!("-std={}", std_lower)
    } else if lang_lower == "cpp"
        || lang_lower == "c++"
        || lang_lower == "cxx"
        || lang_lower == "cuda"
        || lang_lower == "cu"
    {
        format!("-std=c++{}", std_lower)
    } else {
        format!("-std=c{}", std_lower)
    };

    let mut cmd = Command::new(compiler_name);
    cmd.arg(&std_arg).arg("-E");
    if compiler_name.contains("nvcc") || lang_lower == "cuda" || lang_lower == "cu" {
        cmd.args(["-x", "cu", "/dev/null", "-o", "/dev/null"]);
    } else if lang_lower == "cpp" || lang_lower == "c++" || lang_lower == "cxx" {
        cmd.args(["-x", "c++", "/dev/null", "-o", "/dev/null"]);
    } else if lang_lower == "fortran" {
        cmd.args(["-x", "f95", "/dev/null", "-o", "/dev/null"]);
    } else {
        cmd.args(["-x", "c", "/dev/null", "-o", "/dev/null"]);
    }

    if let Ok(out) = cmd.output() {
        if out.status.success() {
            return (
                true,
                Some(format!(
                    "Compiler '{}' capability probe verified support for {}",
                    compiler_name, std_arg
                )),
            );
        } else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stderr.contains("cannot find CUDA") || stderr.contains("cannot find libdevice") {
                let detail = stderr
                    .lines()
                    .find(|l| l.contains("cannot find CUDA") || l.contains("cannot find libdevice"))
                    .map(|l| l.trim())
                    .unwrap_or("missing CUDA toolkit");
                return (
                    false,
                    Some(format!(
                        "Compiler '{}' cannot compile CUDA without an installed CUDA toolkit ({})",
                        compiler_name, detail
                    )),
                );
            }
            if stderr.contains("unrecognized command-line option")
                || stderr.contains("invalid value")
                || stderr.contains("unknown argument")
                || stderr.contains("error: invalid")
                || stderr.contains("not recognized")
            {
                return (
                    false,
                    Some(format!(
                        "Compiler '{}' does not support standard flag {}",
                        compiler_name, std_arg
                    )),
                );
            }
        }
    }

    // 2. Static version matrix fallback across standards
    if let Some(ver) = tool_version {
        let is_gcc = compiler_name.contains("gcc") || compiler_name.contains("g++");
        let is_clang = compiler_name.contains("clang");
        let is_nvcc = compiler_name.contains("nvcc");
        let is_gfortran = compiler_name.contains("gfortran");

        if lang_lower == "c" {
            let min_gcc = match std_lower.as_str() {
                "c99" | "gnu99" | "99" => Some("3.0.0"),
                "c11" | "gnu11" | "11" => Some("4.9.0"),
                "c17" | "gnu17" | "c18" | "gnu18" | "17" | "18" => Some("8.1.0"),
                "c23" | "gnu23" | "23" => Some("14.0.0"),
                _ => None,
            };
            let min_clang = match std_lower.as_str() {
                "c99" | "gnu99" | "99" => Some("1.0.0"),
                "c11" | "gnu11" | "11" => Some("3.1.0"),
                "c17" | "gnu17" | "c18" | "gnu18" | "17" | "18" => Some("6.0.0"),
                "c23" | "gnu23" | "23" => Some("18.0.0"),
                _ => None,
            };

            if is_gcc {
                if let Some(min_v) = min_gcc {
                    let c = VersionConstraint::parse(&format!("< {}", min_v));
                    if c.matches(ver) {
                        return (
                            false,
                            Some(format!(
                                "Compiler '{}' version {} is older than minimum version {} required for standard {}",
                                compiler_name, ver, min_v, standard
                            )),
                        );
                    }
                }
            } else if is_clang {
                if let Some(min_v) = min_clang {
                    let c = VersionConstraint::parse(&format!("< {}", min_v));
                    if c.matches(ver) {
                        return (
                            false,
                            Some(format!(
                                "Compiler '{}' version {} is older than minimum version {} required for standard {}",
                                compiler_name, ver, min_v, standard
                            )),
                        );
                    }
                }
            }
        } else if lang_lower == "cpp" || lang_lower == "c++" || lang_lower == "cxx" {
            let min_gcc = match std_lower.as_str() {
                "c++11" | "gnu++11" | "11" => Some("4.8.1"),
                "c++14" | "gnu++14" | "14" => Some("5.0.0"),
                "c++17" | "gnu++17" | "17" => Some("7.0.0"),
                "c++20" | "gnu++20" | "20" => Some("11.0.0"),
                "c++23" | "gnu++23" | "23" => Some("14.0.0"),
                _ => None,
            };
            let min_clang = match std_lower.as_str() {
                "c++11" | "gnu++11" | "11" => Some("3.3.0"),
                "c++14" | "gnu++14" | "14" => Some("3.4.0"),
                "c++17" | "gnu++17" | "17" => Some("5.0.0"),
                "c++20" | "gnu++20" | "20" => Some("10.0.0"),
                "c++23" | "gnu++23" | "23" => Some("17.0.0"),
                _ => None,
            };

            if is_gcc {
                if let Some(min_v) = min_gcc {
                    let c = VersionConstraint::parse(&format!("< {}", min_v));
                    if c.matches(ver) {
                        return (
                            false,
                            Some(format!(
                                "C++ compiler '{}' version {} is older than minimum version {} required for standard {}",
                                compiler_name, ver, min_v, standard
                            )),
                        );
                    }
                }
            } else if is_clang {
                if let Some(min_v) = min_clang {
                    let c = VersionConstraint::parse(&format!("< {}", min_v));
                    if c.matches(ver) {
                        return (
                            false,
                            Some(format!(
                                "C++ compiler '{}' version {} is older than minimum version {} required for standard {}",
                                compiler_name, ver, min_v, standard
                            )),
                        );
                    }
                }
            }
        } else if lang_lower == "cuda" || lang_lower == "cu" {
            let min_nvcc = match std_lower.as_str() {
                "c++11" | "gnu++11" | "11" => Some("7.0.0"),
                "c++14" | "gnu++14" | "14" => Some("9.0.0"),
                "c++17" | "gnu++17" | "17" => Some("11.0.0"),
                "c++20" | "gnu++20" | "20" => Some("12.0.0"),
                "c++23" | "gnu++23" | "23" => Some("12.8.0"),
                _ => None,
            };
            let min_clang = match std_lower.as_str() {
                "c++11" | "gnu++11" | "11" => Some("4.0.0"),
                "c++14" | "gnu++14" | "14" => Some("6.0.0"),
                "c++17" | "gnu++17" | "17" => Some("9.0.0"),
                "c++20" | "gnu++20" | "20" => Some("13.0.0"),
                _ => None,
            };

            if is_nvcc {
                if let Some(min_v) = min_nvcc {
                    let c = VersionConstraint::parse(&format!("< {}", min_v));
                    if c.matches(ver) {
                        return (
                            false,
                            Some(format!(
                                "CUDA compiler '{}' version {} is older than minimum version {} required for standard {}",
                                compiler_name, ver, min_v, standard
                            )),
                        );
                    }
                }
            } else if is_clang {
                if let Some(min_v) = min_clang {
                    let c = VersionConstraint::parse(&format!("< {}", min_v));
                    if c.matches(ver) {
                        return (
                            false,
                            Some(format!(
                                "CUDA compiler '{}' version {} is older than minimum version {} required for standard {}",
                                compiler_name, ver, min_v, standard
                            )),
                        );
                    }
                }
            }
        } else if lang_lower == "fortran" {
            let min_gfortran = match std_lower.as_str() {
                "f95" | "fortran95" | "95" => Some("4.3.0"),
                "f2003" | "fortran2003" | "2003" | "03" => Some("4.5.0"),
                "f2008" | "fortran2008" | "2008" | "08" => Some("4.8.0"),
                "f2018" | "fortran2018" | "2018" | "18" => Some("8.1.0"),
                _ => None,
            };

            if is_gfortran {
                if let Some(min_v) = min_gfortran {
                    let c = VersionConstraint::parse(&format!("< {}", min_v));
                    if c.matches(ver) {
                        return (
                            false,
                            Some(format!(
                                "Fortran compiler '{}' version {} is older than minimum version {} required for standard {}",
                                compiler_name, ver, min_v, standard
                            )),
                        );
                    }
                }
            }
        }
    }

    (true, None)
}

/// Evaluates a single constraint against machine capabilities.
pub fn evaluate_constraint(
    constraint: &Constraint,
    machine: &MachineCapability,
    project_evidence: Option<Evidence>,
) -> EvaluatedConstraint {
    match constraint {
        Constraint::RuntimeVersion {
            runtime,
            constraint: req_constraint,
        } => {
            if let Some(rt) = machine.find_runtime(runtime) {
                if req_constraint.matches(&rt.version) {
                    EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Satisfied,
                        project_evidence,
                        machine_evidence: Some(rt.evidence.clone()),
                    }
                } else {
                    let reason = format!(
                        "Runtime '{}' version {} does not satisfy requirement {}",
                        runtime, rt.version, req_constraint
                    );
                    let root_cause_hint = format!("{}.version_mismatch", runtime);
                    EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Violated {
                            reason,
                            root_cause_hint,
                        },
                        project_evidence,
                        machine_evidence: Some(rt.evidence.clone()),
                    }
                }
            } else {
                let reason = format!("Runtime '{}' is not installed or not in PATH", runtime);
                let root_cause_hint = format!("{}.missing", runtime);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::PackageManagerVersion {
            name,
            constraint: req_constraint,
        } => {
            if let Some(pm) = machine.find_package_manager(name) {
                if let Some(req_c) = req_constraint {
                    if let Some(ref ver) = pm.version {
                        if req_c.matches(ver) {
                            EvaluatedConstraint {
                                constraint: constraint.clone(),
                                status: ConstraintStatus::Satisfied,
                                project_evidence,
                                machine_evidence: Some(pm.evidence.clone()),
                            }
                        } else {
                            let reason = format!(
                                "Package manager '{}' version {} does not satisfy requirement {}",
                                name, ver, req_c
                            );
                            let root_cause_hint = format!("{}.version_mismatch", name);
                            EvaluatedConstraint {
                                constraint: constraint.clone(),
                                status: ConstraintStatus::Violated {
                                    reason,
                                    root_cause_hint,
                                },
                                project_evidence,
                                machine_evidence: Some(pm.evidence.clone()),
                            }
                        }
                    } else {
                        EvaluatedConstraint {
                            constraint: constraint.clone(),
                            status: ConstraintStatus::Satisfied,
                            project_evidence,
                            machine_evidence: Some(pm.evidence.clone()),
                        }
                    }
                } else {
                    EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Satisfied,
                        project_evidence,
                        machine_evidence: Some(pm.evidence.clone()),
                    }
                }
            } else {
                let reason = format!("Package manager '{}' is not installed or not in PATH", name);
                let root_cause_hint = format!("{}.missing", name);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::ToolAvailable {
            name,
            kind,
            constraint: req_constraint,
            scope,
        } => {
            if let Some(tool) = machine.find_tool(name) {
                if let Some(req_c) = req_constraint {
                    if let Some(ref ver) = tool.version {
                        if req_c.matches(ver) {
                            EvaluatedConstraint {
                                constraint: constraint.clone(),
                                status: ConstraintStatus::Satisfied,
                                project_evidence,
                                machine_evidence: Some(tool.evidence.clone()),
                            }
                        } else {
                            let reason = format!(
                                "{} '{}' version {} does not satisfy requirement {}",
                                kind, name, ver, req_c
                            );
                            let root_cause_hint = format!("{}.version_mismatch", name);
                            EvaluatedConstraint {
                                constraint: constraint.clone(),
                                status: ConstraintStatus::Violated {
                                    reason,
                                    root_cause_hint,
                                },
                                project_evidence,
                                machine_evidence: Some(tool.evidence.clone()),
                            }
                        }
                    } else {
                        EvaluatedConstraint {
                            constraint: constraint.clone(),
                            status: ConstraintStatus::Satisfied,
                            project_evidence,
                            machine_evidence: Some(tool.evidence.clone()),
                        }
                    }
                } else {
                    EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Satisfied,
                        project_evidence,
                        machine_evidence: Some(tool.evidence.clone()),
                    }
                }
            } else {
                let reason = format!(
                    "{} '{}' is not installed or not in PATH (scope: {:?})",
                    kind, name, scope
                );
                let root_cause_hint = format!("{}.missing", name);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::PortAvailable { port } => {
            if let Some(occupied_info) = machine.is_port_occupied(*port) {
                let reason = format!(
                    "Required port {} is already in use by another process",
                    port
                );
                let root_cause_hint = format!("port:{}.occupied", port);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: Some(occupied_info.evidence.clone()),
                }
            } else {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::ServiceRunning {
            service,
            min_version,
        } => {
            if let Some(srv) = machine.find_service(service) {
                if srv.status == ServiceStatus::Running {
                    if let (Some(req_ver), Some(actual_ver)) = (min_version, &srv.version) {
                        if matches_version_constraint(actual_ver, &format!(">={}", req_ver)) {
                            EvaluatedConstraint {
                                constraint: constraint.clone(),
                                status: ConstraintStatus::Satisfied,
                                project_evidence,
                                machine_evidence: Some(srv.evidence.clone()),
                            }
                        } else {
                            let reason = format!(
                                "Service '{}' version {} is older than required version {}",
                                service, actual_ver, req_ver
                            );
                            let root_cause_hint = format!("{}.version_incompatible", service);
                            EvaluatedConstraint {
                                constraint: constraint.clone(),
                                status: ConstraintStatus::Violated {
                                    reason,
                                    root_cause_hint,
                                },
                                project_evidence,
                                machine_evidence: Some(srv.evidence.clone()),
                            }
                        }
                    } else {
                        EvaluatedConstraint {
                            constraint: constraint.clone(),
                            status: ConstraintStatus::Satisfied,
                            project_evidence,
                            machine_evidence: Some(srv.evidence.clone()),
                        }
                    }
                } else {
                    let reason = format!(
                        "Service '{}' is present but currently stopped/inactive",
                        service
                    );
                    let root_cause_hint = format!("{}.stopped", service);
                    EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Violated {
                            reason,
                            root_cause_hint,
                        },
                        project_evidence,
                        machine_evidence: Some(srv.evidence.clone()),
                    }
                }
            } else {
                let reason = format!("Required service '{}' is not installed", service);
                let root_cause_hint = format!("{}.not_installed", service);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::OsMatch { expected_os } => {
            let matches = machine
                .os
                .to_lowercase()
                .contains(&expected_os.to_lowercase())
                || machine.os_family.eq_ignore_ascii_case(expected_os);
            if matches {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            } else {
                let reason = format!(
                    "Operating system mismatch: expected '{}', machine is '{}'",
                    expected_os, machine.os
                );
                let root_cause_hint = "os.mismatch".to_string();
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::ArchMatch { expected_arch } => {
            if machine.arch.eq_ignore_ascii_case(expected_arch) {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            } else {
                let reason = format!(
                    "Architecture mismatch: expected '{}', machine is '{}'",
                    expected_arch, machine.arch
                );
                let root_cause_hint = "arch.mismatch".to_string();
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::MemoryMin { min_bytes } => {
            if machine.total_memory_bytes >= *min_bytes {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            } else {
                let reason = format!(
                    "Insufficient memory: required {:.1} GB, machine has {:.1} GB",
                    *min_bytes as f64 / (1024.0 * 1024.0 * 1024.0),
                    machine.total_memory_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
                );
                let root_cause_hint = "memory.insufficient".to_string();
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::EnvVarSet { key, required } => {
            if machine.env_vars.contains_key(key) {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            } else if *required {
                let reason = format!("Required environment variable '{}' is not set", key);
                let root_cause_hint = format!("env.{}.missing", key);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            } else {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::ConflictDetected { target, details } => {
            let reason = format!(
                "Contradictory configuration detected for '{}': {}",
                target, details
            );
            let root_cause_hint = format!("{}.configuration_conflict", target);
            EvaluatedConstraint {
                constraint: constraint.clone(),
                status: ConstraintStatus::Violated {
                    reason,
                    root_cause_hint,
                },
                project_evidence,
                machine_evidence: None,
            }
        }

        Constraint::ComposeConfigUnresolved {
            compose_file,
            project_name: _,
            service_name,
            missing_env_files,
            unresolved_vars,
            env_templates: _,
            directly_affected_services: _,
            transitively_blocked_services: _,
            bootstrap_suggestions: _,
        } => {
            let mut reasons = Vec::new();
            if !missing_env_files.is_empty() {
                let files: Vec<String> = missing_env_files
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect();
                reasons.push(format!(
                    "required env file(s) missing: {}",
                    files.join(", ")
                ));
            }
            if !unresolved_vars.is_empty() {
                reasons.push(format!(
                    "unresolved required variable(s): {}",
                    unresolved_vars.join(", ")
                ));
            }
            let svc_str = service_name
                .as_deref()
                .map(|s| format!(" for service '{}'", s))
                .unwrap_or_default();
            let reason = format!(
                "Docker Compose configuration in '{}'{} is unresolved: {}",
                compose_file.display(),
                svc_str,
                reasons.join("; ")
            );
            let root_cause_hint = service_name
                .as_deref()
                .map(|s| format!("compose.{}.config_unresolved", s))
                .unwrap_or_else(|| "compose.config_unresolved".to_string());
            EvaluatedConstraint {
                constraint: constraint.clone(),
                status: ConstraintStatus::Violated {
                    reason,
                    root_cause_hint,
                },
                project_evidence,
                machine_evidence: None,
            }
        }

        Constraint::ComposeServiceState {
            compose_file,
            service_name,
            container_name,
            expected_state,
            actual_state,
        } => {
            let satisfied = match (expected_state.as_str(), actual_state.as_str()) {
                ("running", "running") | ("running", "running (healthy)") => true,
                ("healthy", "running (healthy)") => true,
                _ => actual_state == expected_state,
            };
            if satisfied {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            } else {
                let c_str = container_name
                    .as_deref()
                    .map(|c| format!(" (container '{}')", c))
                    .unwrap_or_default();
                let reason = format!(
                    "Compose service '{}'{} defined in '{}' is in state '{}', expected '{}'",
                    service_name,
                    c_str,
                    compose_file.display(),
                    actual_state,
                    expected_state
                );
                let root_cause_hint = format!("compose.{}.state_mismatch", service_name);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::CompilerAvailable {
            language,
            min_standard,
            constraint: req_constraint,
        } => {
            let candidates: &[&str] = match language.to_lowercase().as_str() {
                "c" => &["gcc", "clang", "cc"],
                "cpp" | "c++" | "cxx" => &["g++", "clang++", "c++", "gcc", "clang"],
                "cuda" | "cu" => &["nvcc", "clang++", "clang"],
                "fortran" => &["gfortran", "flang"],
                "rust" => &["rustc"],
                _ => &[language.as_str()],
            };

            let mut found_tool = None;
            for cand in candidates {
                if let Some(tool) = machine.find_tool(cand) {
                    found_tool = Some(tool.clone());
                    break;
                }
            }

            if let Some(tool) = found_tool {
                let mut satisfied = true;
                let mut fail_reason = None;

                if let Some(req_c) = req_constraint {
                    if let Some(ref ver) = tool.version {
                        if !req_c.matches(ver) {
                            satisfied = false;
                            fail_reason = Some(format!(
                                "Compiler '{}' version {} does not satisfy requirement {}",
                                tool.name, ver, req_c
                            ));
                        }
                    }
                }

                if satisfied {
                    if let Some(ref std_name) = min_standard {
                        let (supported, detail) = compiler_supports_standard(
                            &tool.name,
                            language,
                            std_name,
                            tool.version.as_deref(),
                        );
                        if !supported {
                            satisfied = false;
                            fail_reason = detail;
                        }
                    }
                }

                if satisfied {
                    EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Satisfied,
                        project_evidence,
                        machine_evidence: Some(tool.evidence.clone()),
                    }
                } else {
                    let reason = fail_reason.unwrap_or_else(|| {
                        format!("Compiler '{}' does not satisfy requirements", tool.name)
                    });
                    let root_cause_hint = format!("{}.compiler_incompatible", language);
                    EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Violated {
                            reason,
                            root_cause_hint,
                        },
                        project_evidence,
                        machine_evidence: Some(tool.evidence.clone()),
                    }
                }
            } else {
                let reason = format!(
                    "No compiler found for language '{}' (checked: {})",
                    language,
                    candidates.join(", ")
                );
                let root_cause_hint = format!("{}.compiler_missing", language);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::LanguagePackageAvailable {
            language,
            package,
            constraint: _req_constraint,
            scope,
        } => {
            if matches!(scope, ToolScope::Optional | ToolScope::DeclaredButUnused) {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            } else if language == "python" {
                let check_cmd = Command::new("python3")
                    .args(["-c", &format!("import {}", package)])
                    .output();
                match check_cmd {
                    Ok(out) if out.status.success() => EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Satisfied,
                        project_evidence,
                        machine_evidence: None,
                    },
                    _ => {
                        let reason = format!(
                            "Python module '{}' is not installed in the active Python environment",
                            package
                        );
                        let root_cause_hint = format!("python.{}.missing", package);
                        EvaluatedConstraint {
                            constraint: constraint.clone(),
                            status: ConstraintStatus::Violated {
                                reason,
                                root_cause_hint,
                            },
                            project_evidence,
                            machine_evidence: None,
                        }
                    }
                }
            } else {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }

        Constraint::SystemLibraryAvailable {
            name,
            header,
            constraint: req_constraint,
            scope,
        } => {
            if matches!(scope, ToolScope::Optional | ToolScope::DeclaredButUnused) {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            } else {
                let mut found_evidence = None;
                let mut found_version = None;

                // 1. Check pkg-config metadata
                let pkg_names = if let Some(stripped) = name.strip_prefix("lib") {
                    vec![name.clone(), stripped.to_string()]
                } else {
                    vec![name.clone(), format!("lib{}", name)]
                };

                let pkg_config_path = machine
                    .env_vars
                    .get("PKG_CONFIG_PATH")
                    .cloned()
                    .or_else(|| std::env::var("PKG_CONFIG_PATH").ok());

                for pkg in &pkg_names {
                    let mut cmd = Command::new("pkg-config");
                    if let Some(ref pcp) = pkg_config_path {
                        cmd.env("PKG_CONFIG_PATH", pcp);
                    }
                    cmd.args(["--exists", pkg]);

                    if let Ok(out) = cmd.output() {
                        if out.status.success() {
                            let mut ver_discovered = None;
                            let mut ver_cmd = Command::new("pkg-config");
                            if let Some(ref pcp) = pkg_config_path {
                                ver_cmd.env("PKG_CONFIG_PATH", pcp);
                            }
                            ver_cmd.args(["--modversion", pkg]);

                            if let Ok(ver_out) = ver_cmd.output() {
                                if ver_out.status.success() {
                                    let ver =
                                        String::from_utf8_lossy(&ver_out.stdout).trim().to_string();
                                    if !ver.is_empty() {
                                        ver_discovered = Some(ver);
                                    }
                                }
                            }

                            let detail = if let Some(ref ver) = ver_discovered {
                                format!(
                                    "pkg-config metadata exists for '{}' (version: {})",
                                    pkg, ver
                                )
                            } else {
                                format!("pkg-config metadata exists for '{}'", pkg)
                            };

                            let ev = Evidence::new(
                                EvidenceSource::DynamicProbe {
                                    target: format!("pkg-config {}", pkg),
                                    probe_type: "pkg-config".to_string(),
                                    outcome: detail.clone(),
                                },
                                Confidence::Confirmed,
                                detail,
                            );

                            if ver_discovered.is_some() {
                                found_version = ver_discovered;
                                found_evidence = Some(ev);
                                break;
                            } else if found_evidence.is_none() {
                                found_evidence = Some(ev);
                            }
                        }
                    }
                }

                // 2. Check standard linker-discoverable library paths
                if found_evidence.is_none() {
                    let mut lib_dirs: Vec<PathBuf> = Vec::new();
                    if let Some(val) = machine
                        .env_vars
                        .get("LIBRARY_PATH")
                        .or_else(|| machine.env_vars.get("LD_LIBRARY_PATH"))
                    {
                        for p in std::env::split_paths(val) {
                            lib_dirs.push(p);
                        }
                    } else {
                        if let Ok(val) = std::env::var("LIBRARY_PATH") {
                            for p in std::env::split_paths(&val) {
                                lib_dirs.push(p);
                            }
                        }
                        if let Ok(val) = std::env::var("LD_LIBRARY_PATH") {
                            for p in std::env::split_paths(&val) {
                                lib_dirs.push(p);
                            }
                        }
                    }
                    for default_dir in [
                        "/usr/lib",
                        "/usr/lib64",
                        "/usr/lib/x86_64-linux-gnu",
                        "/usr/lib/aarch64-linux-gnu",
                        "/lib",
                        "/lib64",
                        "/usr/local/lib",
                    ] {
                        lib_dirs.push(PathBuf::from(default_dir));
                    }

                    let mut patterns = Vec::new();
                    if name.starts_with("lib") {
                        patterns.push(format!("{}.so", name));
                        patterns.push(format!("{}.a", name));
                    } else {
                        patterns.push(format!("lib{}.so", name));
                        patterns.push(format!("lib{}.a", name));
                        patterns.push(format!("{}.so", name));
                    }

                    for dir in &lib_dirs {
                        for pat in &patterns {
                            let candidate = dir.join(pat);
                            if candidate.exists() {
                                let detail = format!(
                                    "library binary discoverable by linker at '{}'",
                                    candidate.display()
                                );
                                found_evidence = Some(Evidence::new(
                                    EvidenceSource::DirectObservation {
                                        detail: detail.clone(),
                                    },
                                    Confidence::High,
                                    detail,
                                ));
                                break;
                            }
                        }
                        if found_evidence.is_some() {
                            break;
                        }
                    }
                }

                // 3. If header specified, verify header exists
                if let Some(ref hdr) = header {
                    let mut header_dirs = Vec::new();
                    if let Some(val) = machine
                        .env_vars
                        .get("CPATH")
                        .or_else(|| machine.env_vars.get("C_INCLUDE_PATH"))
                    {
                        for p in std::env::split_paths(val) {
                            header_dirs.push(p);
                        }
                    }
                    for default_dir in [
                        "/usr/include",
                        "/usr/local/include",
                        "/usr/include/x86_64-linux-gnu",
                        "/usr/include/aarch64-linux-gnu",
                    ] {
                        header_dirs.push(PathBuf::from(default_dir));
                    }

                    let mut header_path = None;
                    for dir in &header_dirs {
                        let p = dir.join(hdr);
                        if p.exists() {
                            header_path = Some(p);
                            break;
                        }
                    }

                    if let Some(hp) = header_path {
                        if let Some(ref mut ev) = found_evidence {
                            ev.description =
                                format!("{}, header exists at '{}'", ev.description, hp.display());
                        }
                    } else {
                        found_evidence = None;
                        found_version = None;
                    }
                }

                if let Some(ev) = found_evidence {
                    match req_constraint {
                        None => EvaluatedConstraint {
                            constraint: constraint.clone(),
                            status: ConstraintStatus::Satisfied,
                            project_evidence,
                            machine_evidence: Some(ev),
                        },
                        Some(req_c) => {
                            if let Some(ref ver) = found_version {
                                if req_c.matches(ver) {
                                    EvaluatedConstraint {
                                        constraint: constraint.clone(),
                                        status: ConstraintStatus::Satisfied,
                                        project_evidence,
                                        machine_evidence: Some(ev),
                                    }
                                } else {
                                    let reason = format!(
                                        "System library '{}' installed version {} does not satisfy requirement {}",
                                        name, ver, req_c
                                    );
                                    let root_cause_hint =
                                        format!("syslib.{}.version_incompatible", name);
                                    EvaluatedConstraint {
                                        constraint: constraint.clone(),
                                        status: ConstraintStatus::Violated {
                                            reason,
                                            root_cause_hint,
                                        },
                                        project_evidence,
                                        machine_evidence: Some(ev),
                                    }
                                }
                            } else {
                                let reason = format!(
                                    "System library '{}' was found ({}), but its version could not be determined to verify requirement {}",
                                    name, ev.description, req_c
                                );
                                EvaluatedConstraint {
                                    constraint: constraint.clone(),
                                    status: ConstraintStatus::Unknown { reason },
                                    project_evidence,
                                    machine_evidence: Some(ev),
                                }
                            }
                        }
                    }
                } else {
                    let header_clause = header
                        .as_deref()
                        .map(|h| format!(" (header '{}')", h))
                        .unwrap_or_default();
                    let ver_clause = req_constraint
                        .as_ref()
                        .map(|c| format!(" satisfying {}", c))
                        .unwrap_or_default();
                    let reason = format!(
                        "System library '{}'{}{} is not discoverable via pkg-config or standard library paths",
                        name, ver_clause, header_clause
                    );
                    let root_cause_hint = format!("syslib.{}.missing", name);
                    EvaluatedConstraint {
                        constraint: constraint.clone(),
                        status: ConstraintStatus::Violated {
                            reason,
                            root_cause_hint,
                        },
                        project_evidence,
                        machine_evidence: None,
                    }
                }
            }
        }

        Constraint::AnyOf {
            capability,
            constraints,
            scope,
        } => {
            let mut satisfied_alts = Vec::new();
            let mut unknown_alts = Vec::new();
            let mut violated_reasons = Vec::new();

            for sub_c in constraints {
                // Ensure alternative probe tests active presence
                let probe_c = match sub_c {
                    Constraint::SystemLibraryAvailable {
                        name,
                        header,
                        constraint: c,
                        ..
                    } => Constraint::SystemLibraryAvailable {
                        name: name.clone(),
                        header: header.clone(),
                        constraint: c.clone(),
                        scope: ToolScope::RequiredForBuild,
                    },
                    Constraint::ToolAvailable {
                        name,
                        kind,
                        constraint: c,
                        ..
                    } => Constraint::ToolAvailable {
                        name: name.clone(),
                        kind: *kind,
                        constraint: c.clone(),
                        scope: ToolScope::RequiredForBuild,
                    },
                    Constraint::LanguagePackageAvailable {
                        language,
                        package,
                        constraint: c,
                        ..
                    } => Constraint::LanguagePackageAvailable {
                        language: language.clone(),
                        package: package.clone(),
                        constraint: c.clone(),
                        scope: ToolScope::RequiredForBuild,
                    },
                    other => other.clone(),
                };

                let eval = evaluate_constraint(&probe_c, machine, None);
                match &eval.status {
                    ConstraintStatus::Satisfied => {
                        satisfied_alts.push((sub_c, eval));
                    }
                    ConstraintStatus::Unknown { reason } => {
                        unknown_alts.push((sub_c, reason.clone()));
                    }
                    ConstraintStatus::Violated { reason, .. } => {
                        violated_reasons.push(format!("{}: {}", sub_c, reason));
                    }
                }
            }

            if !satisfied_alts.is_empty() {
                let first_ev = satisfied_alts
                    .first()
                    .and_then(|(_, e)| e.machine_evidence.clone());
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: first_ev,
                }
            } else if matches!(scope, ToolScope::Optional | ToolScope::DeclaredButUnused) {
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Satisfied,
                    project_evidence,
                    machine_evidence: None,
                }
            } else if !unknown_alts.is_empty() {
                let reason = format!(
                    "Capability '{}' status is unknown: cannot verify providers ({})",
                    capability,
                    unknown_alts
                        .iter()
                        .map(|(c, r)| format!("{}: {}", c, r))
                        .collect::<Vec<_>>()
                        .join("; ")
                );
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Unknown { reason },
                    project_evidence,
                    machine_evidence: None,
                }
            } else {
                let reason = format!(
                    "No provider satisfied for capability '{}' (scope: {:?}). Tried: [{}]",
                    capability,
                    scope,
                    violated_reasons.join(" | ")
                );
                let root_cause_hint = format!("capability.{}.unsatisfied", capability);
                EvaluatedConstraint {
                    constraint: constraint.clone(),
                    status: ConstraintStatus::Violated {
                        reason,
                        root_cause_hint,
                    },
                    project_evidence,
                    machine_evidence: None,
                }
            }
        }
    }
}

/// Convert a ProjectRequirement into a Constraint with optional project manifest context.
pub fn requirement_to_constraint_with_project(
    req: &ProjectRequirement,
    project: Option<&concord_core::ir::ProjectManifest>,
    machine: &MachineCapability,
) -> Option<Constraint> {
    if let Some(ref plat) = req.platform {
        if !machine.os.to_lowercase().contains(&plat.to_lowercase()) {
            return None;
        }
    }
    match &req.kind {
        RequirementKind::Service { name, min_version } => {
            if let Some(proj) = project {
                if let Some((compose_proj, compose_svc)) =
                    proj.find_compose_service_for_service(name)
                {
                    if !compose_proj.can_instantiate {
                        let bootstrap_suggestions = proj
                            .bootstrap_actions
                            .iter()
                            .map(|b| b.description.clone())
                            .collect();
                        return Some(Constraint::ComposeConfigUnresolved {
                            compose_file: compose_proj.file_path.clone(),
                            project_name: compose_proj.name.clone(),
                            service_name: None,
                            missing_env_files: compose_proj.missing_env_files.clone(),
                            unresolved_vars: compose_proj.unresolved_env_vars.clone(),
                            env_templates: compose_proj.env_templates.clone(),
                            directly_affected_services: compose_proj
                                .directly_affected_services
                                .clone(),
                            transitively_blocked_services: compose_proj
                                .transitively_blocked_services
                                .clone(),
                            bootstrap_suggestions,
                        });
                    }

                    let target_container = machine.find_container_for_compose_service(
                        compose_proj.name.as_deref(),
                        &compose_svc.name,
                        compose_svc.container_name.as_deref(),
                    );

                    let (expected_state, actual_state) = if let Some(container) = target_container {
                        let expected = if compose_svc.has_healthcheck {
                            "healthy".to_string()
                        } else {
                            "running".to_string()
                        };
                        let actual = container.status.to_string();
                        (expected, actual)
                    } else {
                        ("running".to_string(), "not-created".to_string())
                    };

                    return Some(Constraint::ComposeServiceState {
                        compose_file: compose_proj.file_path.clone(),
                        service_name: compose_svc.name.clone(),
                        container_name: compose_svc.container_name.clone(),
                        expected_state,
                        actual_state,
                    });
                }
            }

            Some(Constraint::ServiceRunning {
                service: name.clone(),
                min_version: min_version.clone(),
            })
        }
        RequirementKind::AnyOf {
            capability,
            alternatives,
            scope,
        } => {
            let sub_constraints: Vec<Constraint> = alternatives
                .iter()
                .filter_map(|alt| requirement_to_constraint_with_project(alt, project, machine))
                .collect();
            if sub_constraints.is_empty() {
                None
            } else {
                Some(Constraint::AnyOf {
                    capability: capability.clone(),
                    constraints: sub_constraints,
                    scope: *scope,
                })
            }
        }
        _ => requirement_to_constraint(req),
    }
}

/// Evaluates all project requirements against machine capabilities with project manifest context.
pub fn evaluate_project(
    project: &concord_core::ir::ProjectManifest,
    machine: &MachineCapability,
) -> Vec<EvaluatedConstraint> {
    let mut results = Vec::new();
    for compose_proj in &project.compose_projects {
        if !compose_proj.can_instantiate {
            let bootstrap_suggestions = project
                .bootstrap_actions
                .iter()
                .map(|b| b.description.clone())
                .collect();
            let constraint = Constraint::ComposeConfigUnresolved {
                compose_file: compose_proj.file_path.clone(),
                project_name: compose_proj.name.clone(),
                service_name: None,
                missing_env_files: compose_proj.missing_env_files.clone(),
                unresolved_vars: compose_proj.unresolved_env_vars.clone(),
                env_templates: compose_proj.env_templates.clone(),
                directly_affected_services: compose_proj.directly_affected_services.clone(),
                transitively_blocked_services: compose_proj.transitively_blocked_services.clone(),
                bootstrap_suggestions,
            };
            if !results
                .iter()
                .any(|e: &EvaluatedConstraint| e.constraint == constraint)
            {
                let ev = Evidence::from_repo_file(
                    compose_proj.file_path.clone(),
                    None,
                    format!(
                        "Compose file {} cannot be instantiated",
                        compose_proj.file_path.display()
                    ),
                );
                let evaluated = evaluate_constraint(&constraint, machine, Some(ev));
                results.push(evaluated);
            }
        }
    }
    for req in &project.requirements {
        if let Some(constraint) =
            requirement_to_constraint_with_project(req, Some(project), machine)
        {
            if results
                .iter()
                .any(|e: &EvaluatedConstraint| e.constraint == constraint)
            {
                continue;
            }
            let evaluated = evaluate_constraint(&constraint, machine, Some(req.evidence.clone()));
            results.push(evaluated);
        }
    }
    results
}

/// Evaluates all project requirements against machine capabilities.
pub fn evaluate_all(
    requirements: &[ProjectRequirement],
    machine: &MachineCapability,
) -> Vec<EvaluatedConstraint> {
    let mut results = Vec::new();
    for req in requirements {
        if let Some(constraint) = requirement_to_constraint(req) {
            if results
                .iter()
                .any(|e: &EvaluatedConstraint| e.constraint == constraint)
            {
                continue;
            }
            let evaluated = evaluate_constraint(&constraint, machine, Some(req.evidence.clone()));
            results.push(evaluated);
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use concord_core::ir::{Runtime, ToolObservation};

    #[test]
    fn test_anyof_satisfied_when_one_alternative_satisfied() {
        let mut machine = MachineCapability::empty();
        machine.runtimes.push(Runtime {
            name: "python".to_string(),
            version: "3.11.0".to_string(),
            executable_path: std::path::PathBuf::from("/usr/bin/python3"),
            evidence: Evidence::new(
                EvidenceSource::DirectObservation {
                    detail: "test".to_string(),
                },
                Confidence::High,
                "python 3.11".to_string(),
            ),
        });

        let constraint = Constraint::AnyOf {
            capability: "scripting-runtime".to_string(),
            constraints: vec![
                Constraint::RuntimeVersion {
                    runtime: "ruby".to_string(),
                    constraint: VersionConstraint::Any,
                },
                Constraint::RuntimeVersion {
                    runtime: "python".to_string(),
                    constraint: VersionConstraint::parse(">= 3.10"),
                },
            ],
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert_eq!(eval.status, ConstraintStatus::Satisfied);
        assert!(eval.machine_evidence.is_some());
    }

    #[test]
    fn test_anyof_violated_when_all_alternatives_violated() {
        let machine = MachineCapability::empty();

        let constraint = Constraint::AnyOf {
            capability: "scripting-runtime".to_string(),
            constraints: vec![
                Constraint::RuntimeVersion {
                    runtime: "ruby".to_string(),
                    constraint: VersionConstraint::Any,
                },
                Constraint::RuntimeVersion {
                    runtime: "lua".to_string(),
                    constraint: VersionConstraint::Any,
                },
            ],
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_violated());
        if let ConstraintStatus::Violated {
            reason,
            root_cause_hint,
        } = eval.status
        {
            assert!(root_cause_hint.contains("capability.scripting-runtime.unsatisfied"));
            assert!(reason.contains("Tried:"));
        } else {
            panic!("expected violated status");
        }
    }

    #[test]
    fn test_anyof_optional_scope_satisfied_even_if_all_fail() {
        let machine = MachineCapability::empty();

        let constraint = Constraint::AnyOf {
            capability: "optional-helper".to_string(),
            constraints: vec![
                Constraint::RuntimeVersion {
                    runtime: "nonexistent1".to_string(),
                    constraint: VersionConstraint::Any,
                },
                Constraint::RuntimeVersion {
                    runtime: "nonexistent2".to_string(),
                    constraint: VersionConstraint::Any,
                },
            ],
            scope: ToolScope::Optional,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert_eq!(eval.status, ConstraintStatus::Satisfied);
    }

    #[test]
    fn test_syslib_no_version_constraint_satisfied() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pc_content =
            "Name: mockalpha\nDescription: Mock Alpha\nVersion: 1.5.0\nLibs: -lmockalpha\n";
        std::fs::write(temp_dir.path().join("mockalpha.pc"), pc_content).unwrap();

        let mut machine = MachineCapability::empty();
        machine.env_vars.insert(
            "PKG_CONFIG_PATH".to_string(),
            temp_dir.path().display().to_string(),
        );

        let constraint = Constraint::SystemLibraryAvailable {
            name: "mockalpha".to_string(),
            header: None,
            constraint: None,
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert_eq!(eval.status, ConstraintStatus::Satisfied);
        assert!(eval.machine_evidence.is_some());
    }

    #[test]
    fn test_syslib_version_requirement_satisfied() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pc_content =
            "Name: mockbeta\nDescription: Mock Beta\nVersion: 2.4.1\nLibs: -lmockbeta\n";
        std::fs::write(temp_dir.path().join("mockbeta.pc"), pc_content).unwrap();

        let mut machine = MachineCapability::empty();
        machine.env_vars.insert(
            "PKG_CONFIG_PATH".to_string(),
            temp_dir.path().display().to_string(),
        );

        let constraint = Constraint::SystemLibraryAvailable {
            name: "mockbeta".to_string(),
            header: None,
            constraint: Some(VersionConstraint::parse(">= 2.0")),
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert_eq!(eval.status, ConstraintStatus::Satisfied);
        assert!(eval.machine_evidence.is_some());
    }

    #[test]
    fn test_syslib_version_requirement_violated() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pc_content =
            "Name: mockgamma\nDescription: Mock Gamma\nVersion: 1.2.0\nLibs: -lmockgamma\n";
        std::fs::write(temp_dir.path().join("mockgamma.pc"), pc_content).unwrap();

        let mut machine = MachineCapability::empty();
        machine.env_vars.insert(
            "PKG_CONFIG_PATH".to_string(),
            temp_dir.path().display().to_string(),
        );

        let constraint = Constraint::SystemLibraryAvailable {
            name: "mockgamma".to_string(),
            header: None,
            constraint: Some(VersionConstraint::parse(">= 2.0")),
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_violated());
        if let ConstraintStatus::Violated {
            reason,
            root_cause_hint,
        } = eval.status
        {
            assert_eq!(root_cause_hint, "syslib.mockgamma.version_incompatible");
            assert!(reason.contains("installed version 1.2.0 does not satisfy requirement >=2.0"));
        } else {
            panic!("expected Violated status");
        }
    }

    #[test]
    fn test_syslib_version_unknown() {
        let lib_dir = tempfile::tempdir().unwrap();
        let empty_pc_dir = tempfile::tempdir().unwrap();
        std::fs::write(lib_dir.path().join("libmockdelta_unversioned.so"), b"").unwrap();

        let mut machine = MachineCapability::empty();
        machine.env_vars.insert(
            "LIBRARY_PATH".to_string(),
            lib_dir.path().display().to_string(),
        );
        machine.env_vars.insert(
            "PKG_CONFIG_PATH".to_string(),
            empty_pc_dir.path().display().to_string(),
        );

        let constraint = Constraint::SystemLibraryAvailable {
            name: "mockdelta_unversioned".to_string(),
            header: None,
            constraint: Some(VersionConstraint::parse(">= 2.0")),
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_unknown());
        if let ConstraintStatus::Unknown { reason } = eval.status {
            assert!(reason.contains("version could not be determined to verify requirement >=2.0"));
        } else {
            panic!("expected Unknown status");
        }
    }

    #[test]
    fn test_anyof_with_versioned_alternatives() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pc_a = "Name: backend_a\nDescription: Backend A\nVersion: 1.0.0\nLibs: -lbackend_a\n";
        let pc_b = "Name: backend_b\nDescription: Backend B\nVersion: 2.5.0\nLibs: -lbackend_b\n";
        std::fs::write(temp_dir.path().join("backend_a.pc"), pc_a).unwrap();
        std::fs::write(temp_dir.path().join("backend_b.pc"), pc_b).unwrap();

        let mut machine = MachineCapability::empty();
        machine.env_vars.insert(
            "PKG_CONFIG_PATH".to_string(),
            temp_dir.path().display().to_string(),
        );

        let constraint = Constraint::AnyOf {
            capability: "tls-backend".to_string(),
            constraints: vec![
                Constraint::SystemLibraryAvailable {
                    name: "backend_a".to_string(),
                    header: None,
                    constraint: Some(VersionConstraint::parse(">= 2.0")),
                    scope: ToolScope::RequiredForBuild,
                },
                Constraint::SystemLibraryAvailable {
                    name: "backend_b".to_string(),
                    header: None,
                    constraint: Some(VersionConstraint::parse(">= 2.0")),
                    scope: ToolScope::RequiredForBuild,
                },
            ],
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert_eq!(eval.status, ConstraintStatus::Satisfied);
        let ev = eval.machine_evidence.expect("machine evidence");
        assert!(ev.description.contains("backend_b"));
    }

    #[test]
    fn test_anyof_one_provider_compatible_version_satisfied() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pc_a = "Name: prov_a\nDescription: Provider A\nVersion: 0.9.0\nLibs: -lprov_a\n";
        let pc_b = "Name: prov_b\nDescription: Provider B\nVersion: 3.1.0\nLibs: -lprov_b\n";
        std::fs::write(temp_dir.path().join("prov_a.pc"), pc_a).unwrap();
        std::fs::write(temp_dir.path().join("prov_b.pc"), pc_b).unwrap();

        let mut machine = MachineCapability::empty();
        machine.env_vars.insert(
            "PKG_CONFIG_PATH".to_string(),
            temp_dir.path().display().to_string(),
        );

        let constraint = Constraint::AnyOf {
            capability: "compression".to_string(),
            constraints: vec![
                Constraint::SystemLibraryAvailable {
                    name: "missing_provider_xyz".to_string(),
                    header: None,
                    constraint: Some(VersionConstraint::parse(">= 1.0")),
                    scope: ToolScope::RequiredForBuild,
                },
                Constraint::SystemLibraryAvailable {
                    name: "prov_a".to_string(),
                    header: None,
                    constraint: Some(VersionConstraint::parse(">= 2.0")),
                    scope: ToolScope::RequiredForBuild,
                },
                Constraint::SystemLibraryAvailable {
                    name: "prov_b".to_string(),
                    header: None,
                    constraint: Some(VersionConstraint::parse(">= 3.0")),
                    scope: ToolScope::RequiredForBuild,
                },
            ],
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert_eq!(eval.status, ConstraintStatus::Satisfied);
        let ev = eval.machine_evidence.expect("machine evidence");
        assert!(ev.description.contains("prov_b"));
    }

    #[test]
    fn test_anyof_every_provider_incompatible_version_violated() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pc_x = "Name: crypto_x\nDescription: Crypto X\nVersion: 1.0.0\nLibs: -lcrypto_x\n";
        let pc_y = "Name: crypto_y\nDescription: Crypto Y\nVersion: 1.5.0\nLibs: -lcrypto_y\n";
        std::fs::write(temp_dir.path().join("crypto_x.pc"), pc_x).unwrap();
        std::fs::write(temp_dir.path().join("crypto_y.pc"), pc_y).unwrap();

        let mut machine = MachineCapability::empty();
        machine.env_vars.insert(
            "PKG_CONFIG_PATH".to_string(),
            temp_dir.path().display().to_string(),
        );

        let constraint = Constraint::AnyOf {
            capability: "crypto-suite".to_string(),
            constraints: vec![
                Constraint::SystemLibraryAvailable {
                    name: "crypto_x".to_string(),
                    header: None,
                    constraint: Some(VersionConstraint::parse(">= 2.0")),
                    scope: ToolScope::RequiredForBuild,
                },
                Constraint::SystemLibraryAvailable {
                    name: "crypto_y".to_string(),
                    header: None,
                    constraint: Some(VersionConstraint::parse(">= 2.0")),
                    scope: ToolScope::RequiredForBuild,
                },
            ],
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_violated());
        if let ConstraintStatus::Violated {
            reason,
            root_cause_hint,
        } = eval.status
        {
            assert_eq!(root_cause_hint, "capability.crypto-suite.unsatisfied");
            assert!(reason.contains("crypto_x"));
            assert!(reason.contains("crypto_y"));
        } else {
            panic!("expected Violated status");
        }
    }

    #[test]
    fn test_syslib_evidence_contains_discovered_version() {
        let temp_dir = tempfile::tempdir().unwrap();
        let pc_content =
            "Name: mockomega\nDescription: Mock Omega\nVersion: 3.14.15\nLibs: -lmockomega\n";
        std::fs::write(temp_dir.path().join("mockomega.pc"), pc_content).unwrap();

        let mut machine = MachineCapability::empty();
        machine.env_vars.insert(
            "PKG_CONFIG_PATH".to_string(),
            temp_dir.path().display().to_string(),
        );

        let constraint = Constraint::SystemLibraryAvailable {
            name: "mockomega".to_string(),
            header: None,
            constraint: Some(VersionConstraint::parse(">= 3.0")),
            scope: ToolScope::RequiredForBuild,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert_eq!(eval.status, ConstraintStatus::Satisfied);
        let ev = eval.machine_evidence.expect("machine evidence");
        assert!(ev.description.contains("3.14.15"));
        assert_eq!(ev.confidence, Confidence::Confirmed);
    }

    #[test]
    fn test_cuda_compiler_evaluation_satisfied() {
        let mut machine = MachineCapability::empty();
        machine.tools.push(ToolObservation {
            name: "nvcc".to_string(),
            kind: ToolKind::BuildTool,
            version: Some("12.3.107".to_string()),
            executable_path: std::path::PathBuf::from("/usr/local/cuda/bin/nvcc"),
            evidence: Evidence::new(
                EvidenceSource::ExecutableInspection {
                    path: std::path::PathBuf::from("/usr/local/cuda/bin/nvcc"),
                    version_string: "12.3.107".to_string(),
                    exit_code: 0,
                },
                Confidence::Confirmed,
                "NVCC compiler found",
            ),
        });

        let constraint = Constraint::CompilerAvailable {
            language: "cuda".to_string(),
            min_standard: Some("c++17".to_string()),
            constraint: None,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert_eq!(eval.status, ConstraintStatus::Satisfied);
    }

    #[test]
    fn test_cuda_compiler_evaluation_incompatible_standard() {
        let mut machine = MachineCapability::empty();
        machine.tools.push(ToolObservation {
            name: "nvcc".to_string(),
            kind: ToolKind::BuildTool,
            version: Some("10.2.89".to_string()),
            executable_path: std::path::PathBuf::from("/usr/local/cuda/bin/nvcc"),
            evidence: Evidence::new(
                EvidenceSource::ExecutableInspection {
                    path: std::path::PathBuf::from("/usr/local/cuda/bin/nvcc"),
                    version_string: "10.2.89".to_string(),
                    exit_code: 0,
                },
                Confidence::Confirmed,
                "NVCC compiler found",
            ),
        });

        let constraint = Constraint::CompilerAvailable {
            language: "cuda".to_string(),
            min_standard: Some("c++17".to_string()),
            constraint: None,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_violated());
        if let ConstraintStatus::Violated {
            reason,
            root_cause_hint,
        } = eval.status
        {
            assert_eq!(root_cause_hint, "cuda.compiler_incompatible");
            assert!(reason.contains("older than minimum version 11.0.0"));
        } else {
            panic!("expected Violated status");
        }
    }

    #[test]
    fn test_cuda_compiler_evaluation_missing() {
        let machine = MachineCapability::empty();
        let constraint = Constraint::CompilerAvailable {
            language: "cuda".to_string(),
            min_standard: Some("c++17".to_string()),
            constraint: None,
        };

        let eval = evaluate_constraint(&constraint, &machine, None);
        assert!(eval.is_violated());
        if let ConstraintStatus::Violated {
            reason,
            root_cause_hint,
        } = eval.status
        {
            assert_eq!(root_cause_hint, "cuda.compiler_missing");
            assert!(reason.contains("nvcc, clang++, clang"));
        } else {
            panic!("expected Violated status");
        }
    }
}
