# CONCORD Environment Model Specification

This document specifies the canonical Intermediate Representation (IR), Evidence Model, and Graph Representation used across CONCORD.

---

## 1. Domain Entities

### 1.1 `ProjectManifest`
Represents the discovered requirements and characteristics of a software repository:

```rust
pub struct ProjectManifest {
    pub name: String,
    pub root_path: PathBuf,
    pub languages: Vec<String>,
    pub package_managers: Vec<String>,
    pub requirements: Vec<ProjectRequirement>,
    pub declared_ports: Vec<u16>,
    pub env_vars: Vec<String>,
    pub docker_used: bool,
    pub evidence: Vec<Evidence>,
}
```

### 1.2 `ProjectRequirement`
A single requirement declared by or inferred from project files:

```rust
pub struct ProjectRequirement {
    pub name: String,
    pub kind: RequirementKind,
    pub evidence: Evidence,
}

pub enum RequirementKind {
    Runtime { name: String, constraint: String },
    PackageManager { name: String, constraint: Option<String> },
    Port { port: u16, service_hint: Option<String> },
    Service { name: String, min_version: Option<String> },
    EnvVar { name: String, default_value: Option<String>, required: bool },
    Os { name: String },
    Arch { name: String },
    Memory { min_bytes: u64 },
}
```

### 1.3 `MachineCapability`
Represents the observed state and capabilities of the host machine:

```rust
pub struct MachineCapability {
    pub os: String,
    pub os_family: String,
    pub arch: String,
    pub cpu_count: usize,
    pub total_memory_bytes: u64,
    pub available_memory_bytes: u64,
    pub runtimes: Vec<Runtime>,
    pub services: Vec<Service>,
    pub listening_ports: Vec<PortInfo>,
    pub env_vars: HashMap<String, String>,
    pub path_entries: Vec<PathBuf>,
    pub evidence: Vec<Evidence>,
}
```

### 1.4 `Runtime` & `Service` & `PortInfo`
Observed installed tools, daemons, and socket state:

```rust
pub struct Runtime {
    pub name: String,
    pub version: String,
    pub executable_path: PathBuf,
    pub evidence: Evidence,
}

pub struct Service {
    pub name: String,
    pub version: Option<String>,
    pub status: ServiceStatus, // Running | Stopped | NotInstalled | Unknown
    pub port: Option<u16>,
    pub socket_path: Option<PathBuf>,
    pub evidence: Evidence,
}

pub struct PortInfo {
    pub port: u16,
    pub state: PortState, // Free | Occupied { pid, process_name }
    pub evidence: Evidence,
}
```

---

## 2. Evidence & Confidence Model

Every claim, prediction, and observation is backed by `Evidence`.

### 2.1 Confidence Hierarchy
Confidence levels are strictly ordered:

```text
CONFIRMED (4) > HIGH (3) > MEDIUM (2) > LOW (1) > UNKNOWN (0)
```

- **CONFIRMED**: Directly proven fact (e.g. active kernel TCP_LISTEN socket or executable invocation).
- **HIGH**: Deterministic constraint evaluation with high signal certainty (e.g. `engines.node` vs `node --version`).
- **MEDIUM**: Multi-signal inference (e.g. memory requirements or framework convention).
- **LOW**: Heuristic warning.
- **UNKNOWN**: Insufficient evidence.

### 2.2 Evidence Sources
- `RepositoryFile { path, line, detail }`: Extracted from project files.
- `ExecutableInspection { path, version_string, exit_code }`: Discovered via executable inspection.
- `OsMetadata { key, value }`: OS/system metadata (e.g. `/etc/os-release`).
- `NetworkProbe { target, outcome }`: Probed network/socket state.
- `ProcessInspection { pid, name, cmdline }`: Direct `/proc` inspection.
- `EnvironmentVariable { key, value }`: Machine environment variable.
- `DirectObservation { detail }`: Direct filesystem or runtime observation.

---

## 3. Constraint System

Requirements are transformed into declarative `Constraint` variants:

```rust
pub enum Constraint {
    RuntimeVersion { runtime: String, constraint_str: String },
    PortAvailable { port: u16 },
    ServiceRunning { service: String, min_version: Option<String> },
    OsMatch { expected_os: String },
    ArchMatch { expected_arch: String },
    MemoryMin { min_bytes: u64 },
    EnvVarSet { key: String, required: bool },
}
```

Constraints are evaluated against `MachineCapability` by `concord-constraints`, producing `EvaluatedConstraint` with status:
- `Satisfied`
- `Violated { reason, root_cause_hint }`
- `Unknown { reason }`

---

## 4. Environment Graph

The environment graph links all entities using directed edges:

### Node Types
- `Project`: Root project identity.
- `Machine`: Host operating system and architecture.
- `Requirement`: Declared repository requirement.
- `Runtime`: Installed compiler or interpreter.
- `Service`: System service or container engine.
- `Port`: Host TCP/UDP port.
- `Constraint`: Evaluated constraint and outcome.
- `Evidence`: Provenance node.

### Edge Types
- `Requires`: Project requires a constraint.
- `Provides`: Machine provides a capability.
- `Constrains`: Capability is constrained.
- `EvaluatedAs`: Constraint evaluated against capability.
- `SupportedBy`: Node is supported by an evidence node.
- `Violates`: Machine state violates constraint.
- `Blocks`: Violation blocks component or service.
