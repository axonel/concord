use crate::evidence::Evidence;
use crate::version::VersionConstraint;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Category of tool within a development environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Runtime,
    PackageManager,
    DeveloperTool,
    BuildTool,
    CodeGenerator,
    Service,
}

impl std::fmt::Display for ToolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Runtime => write!(f, "runtime"),
            Self::PackageManager => write!(f, "package manager"),
            Self::DeveloperTool => write!(f, "developer tool"),
            Self::BuildTool => write!(f, "build tool"),
            Self::CodeGenerator => write!(f, "code generator"),
            Self::Service => write!(f, "service"),
        }
    }
}

impl ToolKind {
    /// Classifies a tool binary or package name into its canonical ToolKind.
    pub fn classify(name: &str) -> Self {
        let lower = name.to_lowercase();
        let stripped = lower
            .strip_prefix("npm:")
            .or_else(|| lower.strip_prefix("github:"))
            .unwrap_or(&lower);
        let base = stripped.rsplit('/').next().unwrap_or(stripped);

        if matches!(
            base,
            "bison"
                | "yacc"
                | "byacc"
                | "flex"
                | "lex"
                | "gperf"
                | "ragel"
                | "swig"
                | "protoc"
                | "protobuf"
                | "flatc"
                | "flatbuffers"
                | "capnp"
                | "capnproto"
                | "thrift"
                | "rpcgen"
                | "wayland-scanner"
                | "glib-compile-resources"
                | "glib-mkenums"
                | "glib-genmarshal"
                | "bindgen"
                | "cbindgen"
                | "sqlc"
                | "openapi-generator"
                | "oazapfts"
        ) || base.contains("openapi-generator")
            || base.contains("oazapfts")
        {
            Self::CodeGenerator
        } else if matches!(
            base,
            "make"
                | "cmake"
                | "ninja"
                | "meson"
                | "pkg-config"
                | "pkgconf"
                | "gcc"
                | "clang"
                | "cc"
                | "g++"
                | "clang++"
                | "c++"
                | "nvcc"
                | "gfortran"
                | "flang"
                | "wasm-opt"
                | "binaryen"
        ) {
            Self::BuildTool
        } else {
            Self::DeveloperTool
        }
    }
}

/// Operational scope of a tool requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolScope {
    /// Necessary for core application execution and development.
    RequiredForProject,
    /// Necessary for building/compiling the project or native assets.
    RequiredForBuild,
    /// Needed only for specific scripts, tasks, deployment, or code generation.
    RequiredForTask,
    /// Optional tool.
    Optional,
    /// Declared in configuration files but not invoked in scripts.
    DeclaredButUnused,
    /// Scope could not be deterministically resolved.
    Unknown,
}

impl std::fmt::Display for ToolScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequiredForProject => write!(f, "required for project"),
            Self::RequiredForBuild => write!(f, "required for build"),
            Self::RequiredForTask => write!(f, "required for task"),
            Self::Optional => write!(f, "optional"),
            Self::DeclaredButUnused => write!(f, "declared but unused"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Requirement kind declared by or inferred from a project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RequirementKind {
    /// Language or runtime requirement (e.g. Python >= 3.11, Node == 24.21.0).
    Runtime {
        name: String,
        constraint: VersionConstraint,
    },
    /// Package manager requirement (e.g. pnpm == 11.24.0, bun >= 1.0, uv).
    PackageManager {
        name: String,
        constraint: Option<VersionConstraint>,
    },
    /// Developer or infrastructure tool (e.g. opentofu, terragrunt, extism/cli).
    DeveloperTool {
        name: String,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
    },
    /// Build tool or compiler (e.g. binaryen, cmake, make, ninja).
    BuildTool {
        name: String,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
    },
    /// Code generation tool (e.g. oazapfts, openapi-generator-cli, protoc).
    CodeGenerator {
        name: String,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
    },
    /// TCP or UDP port expected by the project (e.g. 3000, 5432, 8080).
    Port {
        port: u16,
        service_hint: Option<String>,
    },
    /// External service or database (e.g. PostgreSQL, Docker, Redis).
    Service {
        name: String,
        min_version: Option<String>,
    },
    /// Environment variable expected by the application.
    EnvVar {
        name: String,
        default_value: Option<String>,
        required: bool,
    },
    /// Operating system requirement.
    Os { name: String },
    /// CPU architecture requirement.
    Arch { name: String },
    /// Minimum physical or available memory.
    Memory { min_bytes: u64 },
    /// Conflict between multiple configuration sources.
    Conflict {
        target: String,
        details: String,
        competing_sources: Vec<String>,
    },
    /// Compiler for a compiled language (e.g. C, C++, Fortran).
    Compiler {
        language: String,
        min_standard: Option<String>,
        constraint: Option<VersionConstraint>,
    },
    /// Language-specific package or module required at build-time or runtime (e.g. python module jinja2 or requests).
    LanguagePackage {
        language: String,
        package: String,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
    },
    /// Native system library or development package (e.g. openssl, zlib, libpng).
    SystemLibrary {
        name: String,
        header: Option<String>,
        constraint: Option<VersionConstraint>,
        scope: ToolScope,
    },
    /// Disjunctive capability requirement satisfied if at least one alternative requirement is satisfied
    /// (e.g. HTTPS backend provided by OpenSSL OR mbedTLS OR WinHTTP, C compiler provided by GCC OR Clang).
    AnyOf {
        capability: String,
        alternatives: Vec<ProjectRequirement>,
        scope: ToolScope,
    },
}

/// A specific requirement declared by a project, preserving evidence and multi-source provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRequirement {
    pub name: String,
    pub kind: RequirementKind,
    pub evidence: Evidence,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_evidence: Vec<Evidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
}

impl ProjectRequirement {
    pub fn new(name: impl Into<String>, kind: RequirementKind, evidence: Evidence) -> Self {
        Self {
            name: name.into(),
            kind,
            evidence,
            additional_evidence: Vec::new(),
            platform: None,
        }
    }

    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }
}

/// An installed runtime on the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    pub name: String,
    pub version: String,
    pub executable_path: PathBuf,
    pub evidence: Evidence,
}

/// An observed package manager on the host system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManagerObservation {
    pub name: String,
    pub version: Option<String>,
    pub executable_path: PathBuf,
    pub evidence: Evidence,
}

/// An observed developer, build, or codegen tool on the host system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolObservation {
    pub name: String,
    pub kind: ToolKind,
    pub version: Option<String>,
    pub executable_path: PathBuf,
    pub evidence: Evidence,
}

/// Status of a local service or daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatus {
    Running,
    Stopped,
    NotInstalled,
    Unknown,
}

/// A service detected or probed on the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub name: String,
    pub version: Option<String>,
    pub status: ServiceStatus,
    pub port: Option<u16>,
    pub socket_path: Option<PathBuf>,
    pub evidence: Evidence,
}

/// State of a network port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PortState {
    Free,
    Occupied {
        pid: Option<u32>,
        process_name: Option<String>,
    },
}

/// Information about a network port on the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortInfo {
    pub port: u16,
    pub state: PortState,
    pub evidence: Evidence,
}

/// Status of a container observed on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContainerStatus {
    Running { healthy: Option<bool> },
    Exited { exit_code: i32 },
    Created,
    Paused,
    Dead,
    Unknown(String),
}

impl std::fmt::Display for ContainerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running {
                healthy: Some(true),
            } => write!(f, "running (healthy)"),
            Self::Running {
                healthy: Some(false),
            } => write!(f, "running (unhealthy)"),
            Self::Running { healthy: None } => write!(f, "running"),
            Self::Exited { exit_code } => write!(f, "exited ({})", exit_code),
            Self::Created => write!(f, "created"),
            Self::Paused => write!(f, "paused"),
            Self::Dead => write!(f, "dead"),
            Self::Unknown(s) => write!(f, "unknown ({})", s),
        }
    }
}

/// Port mapping for a container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerPortMapping {
    pub host_ip: Option<String>,
    pub host_port: u16,
    pub container_port: u16,
    pub protocol: String,
}

/// An observed container instance on the host machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerObservation {
    pub id: String,
    pub names: Vec<String>,
    pub image: String,
    pub status: ContainerStatus,
    #[serde(default)]
    pub ports: Vec<ContainerPortMapping>,
    pub compose_project: Option<String>,
    pub compose_service: Option<String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    pub evidence: Evidence,
}

/// Complete machine capability discovery.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineCapability {
    pub os: String,
    pub os_family: String,
    pub arch: String,
    pub cpu_count: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub runtimes: Vec<Runtime>,
    pub package_managers: Vec<PackageManagerObservation>,
    pub tools: Vec<ToolObservation>,
    pub services: Vec<Service>,
    #[serde(default)]
    pub containers: Vec<ContainerObservation>,
    pub listening_ports: Vec<PortInfo>,
    pub env_vars: HashMap<String, String>,
    pub path_entries: Vec<PathBuf>,
    pub evidence: Vec<Evidence>,
}

impl Default for MachineCapability {
    fn default() -> Self {
        Self::empty()
    }
}

impl MachineCapability {
    pub fn empty() -> Self {
        Self {
            os: "Linux".to_string(),
            os_family: "linux".to_string(),
            arch: "x86_64".to_string(),
            cpu_count: 4,
            total_memory_bytes: 8 * 1024 * 1024 * 1024,
            available_memory_bytes: 4 * 1024 * 1024 * 1024,
            runtimes: Vec::new(),
            package_managers: Vec::new(),
            tools: Vec::new(),
            services: Vec::new(),
            containers: Vec::new(),
            listening_ports: Vec::new(),
            env_vars: HashMap::new(),
            path_entries: Vec::new(),
            evidence: Vec::new(),
        }
    }

    pub fn find_container_for_compose_service(
        &self,
        project_name: Option<&str>,
        service_name: &str,
        container_name: Option<&str>,
    ) -> Option<&ContainerObservation> {
        for c in &self.containers {
            if let Some(target_name) = container_name {
                let clean_target = target_name.trim_start_matches('/');
                if c.names
                    .iter()
                    .any(|n| n.trim_start_matches('/') == clean_target)
                {
                    return Some(c);
                }
            }
            if let (Some(proj), Some(c_proj), Some(c_srv)) = (
                project_name,
                c.compose_project.as_deref(),
                c.compose_service.as_deref(),
            ) {
                if c_proj.eq_ignore_ascii_case(proj) && c_srv.eq_ignore_ascii_case(service_name) {
                    return Some(c);
                }
            }
        }
        None
    }

    pub fn find_runtime(&self, name: &str) -> Option<&Runtime> {
        self.runtimes
            .iter()
            .find(|r| r.name.eq_ignore_ascii_case(name))
    }

    pub fn find_package_manager(&self, name: &str) -> Option<&PackageManagerObservation> {
        self.package_managers
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn find_tool(&self, name: &str) -> Option<&ToolObservation> {
        self.tools
            .iter()
            .find(|t| t.name.eq_ignore_ascii_case(name))
    }

    pub fn find_service(&self, name: &str) -> Option<&Service> {
        self.services
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn is_port_occupied(&self, port: u16) -> Option<&PortInfo> {
        self.listening_ports
            .iter()
            .find(|p| p.port == port && matches!(p.state, PortState::Occupied { .. }))
    }
}

/// Category of an environment variable in project context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvVarCategory {
    /// Explicitly required for application operation (e.g. no default, or marked required).
    Required,
    /// Has a default or fallback value provided in template or configuration.
    OptionalWithDefault,
    /// Intentionally empty value (e.g. KEY="" or KEY='').
    IntentionallyEmpty,
    /// Optional configuration variable without default.
    Optional,
    /// Present in a local configuration file (.env, .env.local).
    ConfiguredLocal,
}

/// A structured environment variable specification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVarSpec {
    pub name: String,
    pub category: EnvVarCategory,
    pub default_value: Option<String>,
    pub declared_source: Option<PathBuf>,
}

/// A sub-component of a project (e.g. "web", "backend", "api", "cli", "worker").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectComponent {
    pub name: String,
    pub path: PathBuf,
    pub languages: Vec<String>,
    pub package_managers: Vec<String>,
    pub requirements: Vec<ProjectRequirement>,
    pub declared_ports: Vec<u16>,
    pub env_vars: Vec<String>,
}

/// Specification of a service defined within a Docker Compose project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeServiceSpec {
    pub name: String,
    pub container_name: Option<String>,
    pub image: Option<String>,
    pub service_type: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub env_files: Vec<PathBuf>,
    #[serde(default)]
    pub ports: Vec<u16>,
    pub has_healthcheck: bool,
    #[serde(default)]
    pub environment_vars: Vec<String>,
    #[serde(default)]
    pub unresolved_interpolations: Vec<String>,
}

/// Detected template corresponding to a missing environment file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnvFileTemplate {
    pub missing_path: PathBuf,
    pub template_path: PathBuf,
}

/// Deterministic bootstrap action statically discovered from repository setup scripts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootstrapAction {
    pub script_path: PathBuf,
    pub action_type: String,
    pub source_template: PathBuf,
    pub target_file: PathBuf,
    pub description: String,
}

/// Specification of a Docker Compose project discovered in the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComposeProjectSpec {
    pub file_path: PathBuf,
    pub name: Option<String>,
    pub services: Vec<ComposeServiceSpec>,
    #[serde(default)]
    pub env_files: Vec<PathBuf>,
    #[serde(default)]
    pub missing_env_files: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_templates: Vec<EnvFileTemplate>,
    #[serde(default)]
    pub unresolved_env_vars: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub directly_affected_services: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitively_blocked_services: Vec<String>,
    pub can_instantiate: bool,
}

/// Structured manifest of project requirements extracted by the project analyzer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub name: String,
    pub root_path: PathBuf,
    pub languages: Vec<String>,
    pub package_managers: Vec<String>,
    pub requirements: Vec<ProjectRequirement>,
    pub declared_ports: Vec<u16>,
    pub env_vars: Vec<String>,
    #[serde(default)]
    pub env_var_specs: Vec<EnvVarSpec>,
    #[serde(default)]
    pub components: Vec<ProjectComponent>,
    #[serde(default)]
    pub compose_projects: Vec<ComposeProjectSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bootstrap_actions: Vec<BootstrapAction>,
    pub docker_used: bool,
    pub evidence: Vec<Evidence>,
}

impl ProjectManifest {
    pub fn empty(name: impl Into<String>, root_path: PathBuf) -> Self {
        Self {
            name: name.into(),
            root_path,
            languages: Vec::new(),
            package_managers: Vec::new(),
            requirements: Vec::new(),
            declared_ports: Vec::new(),
            env_vars: Vec::new(),
            env_var_specs: Vec::new(),
            components: Vec::new(),
            compose_projects: Vec::new(),
            bootstrap_actions: Vec::new(),
            docker_used: false,
            evidence: Vec::new(),
        }
    }

    pub fn find_compose_service_for_service(
        &self,
        service_name: &str,
    ) -> Option<(&ComposeProjectSpec, &ComposeServiceSpec)> {
        for project in &self.compose_projects {
            for svc in &project.services {
                let matches_type = svc
                    .service_type
                    .as_deref()
                    .map(|t| t.eq_ignore_ascii_case(service_name))
                    .unwrap_or(false);
                let matches_name = svc.name.eq_ignore_ascii_case(service_name);
                let matches_image = svc
                    .image
                    .as_deref()
                    .map(|img| img.to_lowercase().contains(&service_name.to_lowercase()))
                    .unwrap_or(false);
                if matches_type || matches_name || matches_image {
                    return Some((project, svc));
                }
            }
        }
        None
    }
}

/// Canonical intermediate representation combining project and machine intelligence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentModel {
    pub project: ProjectManifest,
    pub machine: MachineCapability,
}

impl EnvironmentModel {
    pub fn new(project: ProjectManifest, machine: MachineCapability) -> Self {
        Self { project, machine }
    }
}
