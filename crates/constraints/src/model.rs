use concord_core::evidence::Evidence;
use concord_core::ir::{ToolKind, ToolScope};
use concord_core::version::VersionConstraint;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Declarative environment constraint.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    /// Target runtime must satisfy a version constraint expression.
    RuntimeVersion {
        runtime: String,
        constraint: VersionConstraint,
    },
    /// A package manager must be available and satisfy an optional version constraint.
    PackageManagerVersion {
        name: String,
        constraint: Option<VersionConstraint>,
    },
    /// A developer, build, or codegen tool must be available.
    ToolAvailable {
        name: String,
        kind: ToolKind,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
    },
    /// A required TCP port must not be occupied by another process.
    PortAvailable { port: u16 },
    /// A required system service or daemon must be running.
    ServiceRunning {
        service: String,
        min_version: Option<String>,
    },
    /// The host operating system must match the requirement.
    OsMatch { expected_os: String },
    /// The CPU architecture must match the requirement.
    ArchMatch { expected_arch: String },
    /// Host machine must provide at least the specified memory in bytes.
    MemoryMin { min_bytes: u64 },
    /// An environment variable must be set in the environment.
    EnvVarSet { key: String, required: bool },
    /// Contradictory configuration detected across project specification files.
    ConflictDetected { target: String, details: String },
    /// Docker Compose configuration cannot be instantiated due to missing env files or unresolved variables.
    ComposeConfigUnresolved {
        compose_file: std::path::PathBuf,
        project_name: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        service_name: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        directly_affected_services: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        transitively_blocked_services: Vec<String>,
        missing_env_files: Vec<std::path::PathBuf>,
        unresolved_vars: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        env_templates: Vec<concord_core::ir::EnvFileTemplate>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bootstrap_suggestions: Vec<String>,
    },
    /// A Docker Compose service has container state issues or has not been created.
    ComposeServiceState {
        compose_file: std::path::PathBuf,
        service_name: String,
        container_name: Option<String>,
        expected_state: String,
        actual_state: String,
    },
    /// A compiler for the specified language must be available and satisfy standards/constraints.
    CompilerAvailable {
        language: String,
        min_standard: Option<String>,
        constraint: Option<VersionConstraint>,
    },
    /// A language package/module must be importable in the runtime environment.
    LanguagePackageAvailable {
        language: String,
        package: String,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
    },
    /// A system library or development header must be present on the host.
    SystemLibraryAvailable {
        name: String,
        header: Option<String>,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
    },
    /// Disjunctive capability requirement satisfied if at least one alternative is satisfied.
    AnyOf {
        capability: String,
        constraints: Vec<Constraint>,
        scope: ToolScope,
    },
    /// Build system generation precondition: the generated build artifact must either be present,
    /// or its required generator tool must be available to execute the generation step.
    BuildSystemGenerated {
        input_declaration: std::path::PathBuf,
        generator_tool: String,
        version_constraint: Option<VersionConstraint>,
        generated_artifact: std::path::PathBuf,
        downstream_build_system: String,
        bootstrap_script: Option<std::path::PathBuf>,
        artifact_present: bool,
    },
    /// Constraint guarded by platform and/or architecture conditions.
    EnvironmentGuarded {
        constraint: Box<Constraint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        platform: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arch: Option<String>,
    },
}

impl Constraint {
    /// Unwrap any environment guards down to the underlying concrete constraint.
    pub fn inner_constraint(&self) -> &Constraint {
        match self {
            Self::EnvironmentGuarded { constraint, .. } => constraint.inner_constraint(),
            other => other,
        }
    }

    /// Access the platform and architecture conditions guarding this constraint, if any.
    pub fn environment_guard(&self) -> Option<(&Option<String>, &Option<String>)> {
        match self {
            Self::EnvironmentGuarded { platform, arch, .. } => Some((platform, arch)),
            _ => None,
        }
    }
}

impl fmt::Display for Constraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeVersion {
                runtime,
                constraint,
            } => {
                write!(f, "Runtime '{}' must satisfy {}", runtime, constraint)
            }
            Self::PackageManagerVersion { name, constraint } => {
                if let Some(ref c) = constraint {
                    write!(f, "Package manager '{}' must satisfy {}", name, c)
                } else {
                    write!(f, "Package manager '{}' must be installed", name)
                }
            }
            Self::ToolAvailable {
                name,
                kind,
                constraint,
                scope,
            } => {
                let ver_str = constraint
                    .as_ref()
                    .map(|c| format!(" ({})", c))
                    .unwrap_or_default();
                write!(
                    f,
                    "{} '{}'{} must be available (scope: {:?})",
                    kind, name, ver_str, scope
                )
            }
            Self::CompilerAvailable {
                language,
                min_standard,
                constraint,
            } => {
                let std_str = min_standard
                    .as_ref()
                    .map(|s| format!(" standard {}", s))
                    .unwrap_or_default();
                let ver_str = constraint
                    .as_ref()
                    .map(|c| format!(" version {}", c))
                    .unwrap_or_default();
                write!(
                    f,
                    "Compiler for '{}'{}{} must be available",
                    language, std_str, ver_str
                )
            }
            Self::LanguagePackageAvailable {
                language,
                package,
                constraint,
                scope,
            } => {
                let ver_str = constraint
                    .as_ref()
                    .map(|c| format!(" ({})", c))
                    .unwrap_or_default();
                write!(
                    f,
                    "{} package '{}'{} must be available (scope: {:?})",
                    language, package, ver_str, scope
                )
            }
            Self::SystemLibraryAvailable {
                name,
                header,
                constraint,
                scope,
            } => {
                let header_str = header
                    .as_ref()
                    .map(|h| format!(" with header '{}'", h))
                    .unwrap_or_default();
                let ver_str = constraint
                    .as_ref()
                    .map(|c| format!(" ({})", c))
                    .unwrap_or_default();
                write!(
                    f,
                    "System library '{}'{}{} must be installed (scope: {:?})",
                    name, header_str, ver_str, scope
                )
            }
            Self::PortAvailable { port } => write!(f, "Port {} must be available", port),
            Self::ServiceRunning {
                service,
                min_version,
            } => {
                if let Some(ref ver) = min_version {
                    write!(f, "Service '{}' must be running (>= {})", service, ver)
                } else {
                    write!(f, "Service '{}' must be running", service)
                }
            }
            Self::OsMatch { expected_os } => write!(f, "OS must match '{}'", expected_os),
            Self::ArchMatch { expected_arch } => {
                write!(f, "Architecture must match '{}'", expected_arch)
            }
            Self::MemoryMin { min_bytes } => {
                write!(
                    f,
                    "Available memory must be >= {:.1} GB",
                    *min_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
                )
            }
            Self::EnvVarSet { key, required } => {
                if *required {
                    write!(f, "Environment variable '{}' must be set", key)
                } else {
                    write!(f, "Environment variable '{}' is optional but declared", key)
                }
            }
            Self::ConflictDetected { target, details } => {
                write!(f, "Configuration conflict for '{}': {}", target, details)
            }
            Self::ComposeConfigUnresolved {
                compose_file,
                service_name,
                missing_env_files,
                unresolved_vars,
                ..
            } => {
                let mut issues = Vec::new();
                if !missing_env_files.is_empty() {
                    issues.push(format!(
                        "missing env file(s): {}",
                        missing_env_files
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
                if !unresolved_vars.is_empty() {
                    issues.push(format!(
                        "unresolved variable(s): {}",
                        unresolved_vars.join(", ")
                    ));
                }
                let svc_str = service_name
                    .as_deref()
                    .map(|s| format!(" for service '{}'", s))
                    .unwrap_or_default();
                write!(
                    f,
                    "Docker Compose configuration at '{}'{} must be resolvable ({})",
                    compose_file.display(),
                    svc_str,
                    issues.join("; ")
                )
            }
            Self::ComposeServiceState {
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
                write!(
                    f,
                    "Compose service '{}'{} in '{}' state must be '{}', found '{}'",
                    service_name,
                    c_str,
                    compose_file.display(),
                    expected_state,
                    actual_state
                )
            }
            Self::AnyOf {
                capability,
                constraints,
                scope,
            } => {
                let alts = constraints
                    .iter()
                    .map(|c| format!("{}", c))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                write!(
                    f,
                    "Capability '{}' must be satisfied by at least one of [{}] (scope: {:?})",
                    capability, alts, scope
                )
            }
            Self::BuildSystemGenerated {
                input_declaration,
                generator_tool,
                generated_artifact,
                downstream_build_system,
                bootstrap_script,
                artifact_present,
                ..
            } => {
                let script_str = bootstrap_script
                    .as_ref()
                    .map(|s| format!(" via '{}'", s.display()))
                    .unwrap_or_default();
                let presence_str = if *artifact_present {
                    " (present)"
                } else {
                    " (missing)"
                };
                write!(
                    f,
                    "Build system '{}' artifact '{}'{} must be generated from '{}' using '{}'{}",
                    downstream_build_system,
                    generated_artifact.display(),
                    presence_str,
                    input_declaration.display(),
                    generator_tool,
                    script_str
                )
            }
            Self::EnvironmentGuarded {
                constraint,
                platform,
                arch,
            } => {
                let mut guards = Vec::new();
                if let Some(ref p) = platform {
                    guards.push(format!("platform: {}", p));
                }
                if let Some(ref a) = arch {
                    guards.push(format!("arch: {}", a));
                }
                if guards.is_empty() {
                    write!(f, "{}", constraint)
                } else {
                    write!(f, "{} [guarded by {}]", constraint, guards.join(", "))
                }
            }
        }
    }
}

/// The evaluation outcome of a single constraint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ConstraintStatus {
    Satisfied,
    Violated {
        reason: String,
        root_cause_hint: String,
    },
    Unknown {
        reason: String,
    },
    NotApplicable {
        reason: String,
    },
}

/// Evaluated constraint accompanied by full provenance from both sides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluatedConstraint {
    pub constraint: Constraint,
    pub status: ConstraintStatus,
    pub project_evidence: Option<Evidence>,
    pub machine_evidence: Option<Evidence>,
}

impl EvaluatedConstraint {
    pub fn is_violated(&self) -> bool {
        matches!(self.status, ConstraintStatus::Violated { .. })
    }

    pub fn is_satisfied(&self) -> bool {
        matches!(self.status, ConstraintStatus::Satisfied)
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self.status, ConstraintStatus::Unknown { .. })
    }

    pub fn is_not_applicable(&self) -> bool {
        matches!(self.status, ConstraintStatus::NotApplicable { .. })
    }

    pub fn is_applicable(&self) -> bool {
        !self.is_not_applicable()
    }
}
