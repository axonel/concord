use concord_constraints::model::{Constraint, ConstraintStatus};
use concord_core::evidence::Evidence;
use concord_core::ir::{PortState, RequirementKind, ServiceStatus};
use concord_core::Confidence;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "node_type", rename_all = "snake_case")]
pub enum NodeData {
    Project {
        name: String,
        path: PathBuf,
    },
    Component {
        name: String,
        path: PathBuf,
    },
    Requirement {
        name: String,
        kind: RequirementKind,
    },
    Machine {
        os: String,
        arch: String,
    },
    Runtime {
        name: String,
        version: String,
        executable_path: PathBuf,
    },
    PackageManager {
        name: String,
        version: Option<String>,
        executable_path: PathBuf,
    },
    Tool {
        name: String,
        version: Option<String>,
        executable_path: PathBuf,
    },
    Container {
        name: String,
        image: String,
        status: concord_core::ir::ContainerStatus,
    },
    Service {
        name: String,
        status: ServiceStatus,
        version: Option<String>,
    },
    Port {
        port: u16,
        state: PortState,
    },
    Constraint {
        constraint: Constraint,
        status: ConstraintStatus,
    },
    Evidence {
        description: String,
        confidence: Confidence,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeData {
    ContainsComponent,
    Requires,
    Provides,
    DependsOn,
    TargetsPort,
    UsesRuntime,
    Constrains,
    EvaluatedAs,
    SupportedBy,
    Violates,
    Blocks,
}

/// Structured causal trace linking a violation back to its root requirement, machine state, and evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalTrace {
    pub constraint: Constraint,
    pub status: ConstraintStatus,
    pub requirement: Option<String>,
    pub root_cause: Option<String>,
    pub affected_components: Vec<String>,
    pub causal_steps: Vec<String>,
    pub project_evidence: Option<Evidence>,
    pub machine_state: Option<String>,
    pub machine_evidence: Option<Evidence>,
}
