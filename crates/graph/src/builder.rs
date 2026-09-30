use crate::model::{CausalTrace, EdgeData, NodeData};
use concord_constraints::model::{Constraint, ConstraintStatus, EvaluatedConstraint};
use concord_core::evidence::Evidence;
use concord_core::ir::EnvironmentModel;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::collections::HashMap;

/// The environment graph connecting Project, Components, Requirements, Machine capabilities, Constraints, and Evidence.
#[derive(Debug, Clone)]
pub struct EnvironmentGraph {
    pub graph: DiGraph<NodeData, EdgeData>,
    pub project_node: NodeIndex,
    pub machine_node: NodeIndex,
    pub component_nodes: HashMap<String, NodeIndex>,
}

pub fn language_matches_runtime(lang: &str, runtime: &str) -> bool {
    let lang_lower = lang.to_lowercase();
    let rt_lower = runtime.to_lowercase();
    match rt_lower.as_str() {
        "java" => {
            lang_lower == "java"
                || lang_lower == "kotlin"
                || lang_lower == "scala"
                || lang_lower == "android"
        }
        "node" | "nodejs" | "bun" | "deno" => {
            lang_lower.contains("javascript")
                || lang_lower.contains("typescript")
                || lang_lower == "node"
                || lang_lower == "bun"
        }
        "python" => lang_lower == "python",
        "rust" => lang_lower == "rust",
        "go" | "golang" => lang_lower == "go" || lang_lower == "golang",
        "ruby" => lang_lower == "ruby",
        "php" => lang_lower == "php",
        "dotnet" | "csharp" => {
            lang_lower == "c#" || lang_lower == "csharp" || lang_lower == "dotnet"
        }
        _ => lang_lower == rt_lower,
    }
}

impl EnvironmentGraph {
    /// Construct the environment graph from an EnvironmentModel and evaluated constraints.
    pub fn build(model: &EnvironmentModel, evaluated_constraints: &[EvaluatedConstraint]) -> Self {
        let mut graph = DiGraph::new();

        // 1. Root Project node
        let project_node = graph.add_node(NodeData::Project {
            name: model.project.name.clone(),
            path: model.project.root_path.clone(),
        });

        // 2. Subcomponents
        let mut component_nodes = HashMap::new();
        for comp in &model.project.components {
            let comp_node = graph.add_node(NodeData::Component {
                name: comp.name.clone(),
                path: comp.path.clone(),
            });
            graph.add_edge(project_node, comp_node, EdgeData::ContainsComponent);
            component_nodes.insert(comp.name.clone(), comp_node);
        }

        // 3. Root Machine node
        let machine_node = graph.add_node(NodeData::Machine {
            os: model.machine.os.clone(),
            arch: model.machine.arch.clone(),
        });

        // 4. Machine capabilities (Runtimes, Services, Ports)
        let mut runtime_nodes = HashMap::new();
        for rt in &model.machine.runtimes {
            let rt_node = graph.add_node(NodeData::Runtime {
                name: rt.name.clone(),
                version: rt.version.clone(),
                executable_path: rt.executable_path.clone(),
            });
            graph.add_edge(machine_node, rt_node, EdgeData::Provides);

            let ev_node = graph.add_node(NodeData::Evidence {
                description: rt.evidence.description.clone(),
                confidence: rt.evidence.confidence,
            });
            graph.add_edge(rt_node, ev_node, EdgeData::SupportedBy);
            runtime_nodes.insert(rt.name.to_lowercase(), (rt_node, rt.clone()));
        }

        let mut service_nodes = HashMap::new();
        for srv in &model.machine.services {
            let srv_node = graph.add_node(NodeData::Service {
                name: srv.name.clone(),
                status: srv.status,
                version: srv.version.clone(),
            });
            graph.add_edge(machine_node, srv_node, EdgeData::Provides);

            let ev_node = graph.add_node(NodeData::Evidence {
                description: srv.evidence.description.clone(),
                confidence: srv.evidence.confidence,
            });
            graph.add_edge(srv_node, ev_node, EdgeData::SupportedBy);
            service_nodes.insert(srv.name.to_lowercase(), (srv_node, srv.clone()));
        }

        // 4. Ports
        let mut port_nodes = HashMap::new();
        for p in &model.machine.listening_ports {
            let p_node = graph.add_node(NodeData::Port {
                port: p.port,
                state: p.state.clone(),
            });
            graph.add_edge(machine_node, p_node, EdgeData::Provides);

            let ev_node = graph.add_node(NodeData::Evidence {
                description: p.evidence.description.clone(),
                confidence: p.evidence.confidence,
            });
            graph.add_edge(p_node, ev_node, EdgeData::SupportedBy);
            port_nodes.insert(p.port, (p_node, p.clone()));
        }

        // 5. Package Managers
        let mut pkg_mgr_nodes = HashMap::new();
        for pm in &model.machine.package_managers {
            let pm_node = graph.add_node(NodeData::PackageManager {
                name: pm.name.clone(),
                version: pm.version.clone(),
                executable_path: pm.executable_path.clone(),
            });
            graph.add_edge(machine_node, pm_node, EdgeData::Provides);

            let ev_node = graph.add_node(NodeData::Evidence {
                description: pm.evidence.description.clone(),
                confidence: pm.evidence.confidence,
            });
            graph.add_edge(pm_node, ev_node, EdgeData::SupportedBy);
            pkg_mgr_nodes.insert(pm.name.to_lowercase(), (pm_node, pm.clone()));
        }

        // 6. Developer & Build Tools
        let mut tool_nodes = HashMap::new();
        for tool in &model.machine.tools {
            let tool_node = graph.add_node(NodeData::Tool {
                name: tool.name.clone(),
                version: tool.version.clone(),
                executable_path: tool.executable_path.clone(),
            });
            graph.add_edge(machine_node, tool_node, EdgeData::Provides);

            let ev_node = graph.add_node(NodeData::Evidence {
                description: tool.evidence.description.clone(),
                confidence: tool.evidence.confidence,
            });
            graph.add_edge(tool_node, ev_node, EdgeData::SupportedBy);
            tool_nodes.insert(tool.name.to_lowercase(), (tool_node, tool.clone()));
        }

        // 7. Containers
        let mut container_nodes = HashMap::new();
        for ctr in &model.machine.containers {
            let primary_name = ctr.names.first().cloned().unwrap_or_else(|| ctr.id.clone());
            let ctr_node = graph.add_node(NodeData::Container {
                name: primary_name.clone(),
                image: ctr.image.clone(),
                status: ctr.status.clone(),
            });
            graph.add_edge(machine_node, ctr_node, EdgeData::Provides);

            let ev_node = graph.add_node(NodeData::Evidence {
                description: ctr.evidence.description.clone(),
                confidence: ctr.evidence.confidence,
            });
            graph.add_edge(ctr_node, ev_node, EdgeData::SupportedBy);
            for n in &ctr.names {
                container_nodes.insert(n.clone(), (ctr_node, ctr.clone()));
                let clean = n.trim_start_matches('/');
                container_nodes.insert(clean.to_string(), (ctr_node, ctr.clone()));
            }
            if let Some(ref svc) = ctr.compose_service {
                container_nodes.insert(svc.clone(), (ctr_node, ctr.clone()));
            }
        }

        // 8. Connect known service-to-port dependencies
        if let (Some((pg_node, _)), Some((port_node, _))) =
            (service_nodes.get("postgresql"), port_nodes.get(&5432))
        {
            graph.add_edge(*pg_node, *port_node, EdgeData::TargetsPort);
        }

        // 9. Project Requirements and Constraints
        for eval in evaluated_constraints {
            let constraint_node = graph.add_node(NodeData::Constraint {
                constraint: eval.constraint.clone(),
                status: eval.status.clone(),
            });

            // Associate constraint with matching component if found, otherwise project
            let mut best_match: Option<(NodeIndex, String, u32)> = None;
            for (comp_name, comp_idx) in &component_nodes {
                let comp = model
                    .project
                    .components
                    .iter()
                    .find(|c| &c.name == comp_name);
                if let Some(c) = comp {
                    let mut score = 0u32;

                    // 1. Evidence source path matches component name or directory (highest priority)
                    if let Some(ref p_ev) = eval.project_evidence {
                        if let concord_core::evidence::EvidenceSource::RepositoryFile {
                            path, ..
                        } = &p_ev.source
                        {
                            let p_str = path.to_string_lossy();
                            if p_str.starts_with(comp_name)
                                || p_str.contains(&format!("/{}/", comp_name))
                                || path.starts_with(&c.path)
                            {
                                score += 1000;
                            }
                        }
                    }

                    // 2. Direct component requirement match
                    let has_direct_req = match &eval.constraint {
                        Constraint::RuntimeVersion { runtime, .. } => {
                            c.requirements.iter().any(|r| r.name == *runtime)
                        }
                        Constraint::PackageManagerVersion { name, .. } => {
                            c.requirements.iter().any(|r| r.name == *name)
                        }
                        Constraint::ToolAvailable { name, .. } => {
                            c.requirements.iter().any(|r| r.name == *name)
                        }
                        Constraint::CompilerAvailable { language, .. } => {
                            c.requirements.iter().any(|r| r.name == *language)
                        }
                        Constraint::LanguagePackageAvailable { package, .. } => {
                            c.requirements.iter().any(|r| r.name.contains(package))
                        }
                        Constraint::SystemLibraryAvailable { name, .. } => {
                            c.requirements.iter().any(|r| r.name == *name)
                        }
                        Constraint::PortAvailable { port } => c.declared_ports.contains(port),
                        Constraint::EnvVarSet { key, .. } => c.env_vars.contains(key),
                        Constraint::ComposeServiceState { service_name, .. }
                        | Constraint::ComposeConfigUnresolved {
                            service_name: Some(service_name),
                            ..
                        } => c.requirements.iter().any(|r| r.name == *service_name),
                        Constraint::AnyOf { capability, .. } => {
                            c.requirements.iter().any(|r| r.name == *capability)
                        }
                        _ => false,
                    };
                    if has_direct_req {
                        score += 500;
                    }

                    // 3. Language or package manager heuristic match
                    let matches_heuristic = match &eval.constraint {
                        Constraint::RuntimeVersion { runtime, .. } => c
                            .languages
                            .iter()
                            .any(|l| language_matches_runtime(l, runtime)),
                        Constraint::PackageManagerVersion { name, .. } => c
                            .package_managers
                            .iter()
                            .any(|pm| pm.eq_ignore_ascii_case(name)),
                        _ => false,
                    };
                    if matches_heuristic {
                        score += 100;
                    }

                    if score > 0 {
                        if let Some((_, _, best_score)) = best_match {
                            if score > best_score {
                                best_match = Some((*comp_idx, comp_name.clone(), score));
                            }
                        } else {
                            best_match = Some((*comp_idx, comp_name.clone(), score));
                        }
                    }
                }
            }

            if let Some((comp_idx, _name, _)) = best_match {
                graph.add_edge(comp_idx, constraint_node, EdgeData::Requires);
            } else {
                graph.add_edge(project_node, constraint_node, EdgeData::Requires);
            }

            // Connect project evidence
            if let Some(ref p_ev) = eval.project_evidence {
                let ev_node = graph.add_node(NodeData::Evidence {
                    description: p_ev.description.clone(),
                    confidence: p_ev.confidence,
                });
                graph.add_edge(constraint_node, ev_node, EdgeData::SupportedBy);
            }

            // Connect machine capability and machine evidence
            match &eval.constraint {
                Constraint::RuntimeVersion { runtime, .. } => {
                    if let Some((rt_node, _)) = runtime_nodes.get(&runtime.to_lowercase()) {
                        let edge_type = if eval.is_violated() {
                            EdgeData::Violates
                        } else {
                            EdgeData::EvaluatedAs
                        };
                        graph.add_edge(*rt_node, constraint_node, edge_type);
                    }
                }
                Constraint::PackageManagerVersion { name, .. } => {
                    if let Some((pm_node, _)) = pkg_mgr_nodes.get(&name.to_lowercase()) {
                        let edge_type = if eval.is_violated() {
                            EdgeData::Violates
                        } else {
                            EdgeData::EvaluatedAs
                        };
                        graph.add_edge(*pm_node, constraint_node, edge_type);
                    }
                }
                Constraint::ToolAvailable { name, .. } => {
                    if let Some((t_node, _)) = tool_nodes.get(&name.to_lowercase()) {
                        let edge_type = if eval.is_violated() {
                            EdgeData::Violates
                        } else {
                            EdgeData::EvaluatedAs
                        };
                        graph.add_edge(*t_node, constraint_node, edge_type);
                    }
                }
                Constraint::ComposeServiceState {
                    service_name,
                    container_name,
                    ..
                } => {
                    let target_name = container_name.as_deref().unwrap_or(service_name);
                    if let Some((c_node, _)) = container_nodes.get(target_name) {
                        let edge_type = if eval.is_violated() {
                            EdgeData::Violates
                        } else {
                            EdgeData::EvaluatedAs
                        };
                        graph.add_edge(*c_node, constraint_node, edge_type);
                    }
                }
                Constraint::PortAvailable { port } => {
                    if let Some((p_node, _)) = port_nodes.get(port) {
                        let edge_type = if eval.is_violated() {
                            EdgeData::Violates
                        } else {
                            EdgeData::EvaluatedAs
                        };
                        graph.add_edge(*p_node, constraint_node, edge_type);
                    }
                }
                Constraint::ServiceRunning { service, .. } => {
                    if let Some((srv_node, _)) = service_nodes.get(&service.to_lowercase()) {
                        let edge_type = if eval.is_violated() {
                            EdgeData::Violates
                        } else {
                            EdgeData::EvaluatedAs
                        };
                        graph.add_edge(*srv_node, constraint_node, edge_type);
                    }
                }
                Constraint::AnyOf { constraints, .. } => {
                    for sub_c in constraints {
                        match sub_c {
                            Constraint::RuntimeVersion { runtime, .. } => {
                                if let Some((rt_node, _)) =
                                    runtime_nodes.get(&runtime.to_lowercase())
                                {
                                    let edge_type = if eval.is_violated() {
                                        EdgeData::Violates
                                    } else {
                                        EdgeData::EvaluatedAs
                                    };
                                    graph.add_edge(*rt_node, constraint_node, edge_type);
                                }
                            }
                            Constraint::ToolAvailable { name, .. } => {
                                if let Some((t_node, _)) = tool_nodes.get(&name.to_lowercase()) {
                                    let edge_type = if eval.is_violated() {
                                        EdgeData::Violates
                                    } else {
                                        EdgeData::EvaluatedAs
                                    };
                                    graph.add_edge(*t_node, constraint_node, edge_type);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        Self {
            graph,
            project_node,
            machine_node,
            component_nodes,
        }
    }

    /// Find all constraint nodes that have been violated.
    pub fn find_violations(&self) -> Vec<NodeIndex> {
        let mut violations = Vec::new();
        for idx in self.graph.node_indices() {
            if let Some(NodeData::Constraint { status, .. }) = self.graph.node_weight(idx) {
                if matches!(status, ConstraintStatus::Violated { .. }) {
                    violations.push(idx);
                }
            }
        }
        violations
    }

    /// Trace the causal chain of a violation node back to project components, requirements, and machine observations.
    pub fn trace_causal_chain(&self, violation_idx: NodeIndex) -> Option<CausalTrace> {
        let weight = self.graph.node_weight(violation_idx)?;
        let (constraint, status) = match weight {
            NodeData::Constraint { constraint, status } => (constraint.clone(), status.clone()),
            _ => return None,
        };

        let mut project_evidence: Option<Evidence> = None;
        let mut machine_evidence: Option<Evidence> = None;
        let mut machine_state: Option<String> = None;
        let mut affected_components = Vec::new();

        // 1. Traverse outgoing edges from constraint node (SupportedBy -> Evidence)
        for edge in self
            .graph
            .edges_directed(violation_idx, Direction::Outgoing)
        {
            if *edge.weight() == EdgeData::SupportedBy {
                if let Some(NodeData::Evidence {
                    description,
                    confidence,
                }) = self.graph.node_weight(edge.target())
                {
                    project_evidence = Some(Evidence::new(
                        concord_core::evidence::EvidenceSource::DirectObservation {
                            detail: description.clone(),
                        },
                        *confidence,
                        description.clone(),
                    ));
                    if machine_state.is_none() {
                        machine_state = Some(description.clone());
                    }
                }
            }
        }

        // 2. Traverse incoming edges to find affected component or project
        for edge in self
            .graph
            .edges_directed(violation_idx, Direction::Incoming)
        {
            if *edge.weight() == EdgeData::Requires {
                let source_idx = edge.source();
                if let Some(NodeData::Component { name, .. }) = self.graph.node_weight(source_idx) {
                    if !affected_components.contains(name) {
                        affected_components.push(name.clone());
                    }
                } else if let Some(NodeData::Project { name, .. }) =
                    self.graph.node_weight(source_idx)
                {
                    if affected_components.is_empty() {
                        affected_components.push(name.clone());
                    }
                }
            }
        }

        // 3. Traverse incoming edges from machine capabilities (Runtime, Port, Service)
        for edge in self
            .graph
            .edges_directed(violation_idx, Direction::Incoming)
        {
            match edge.weight() {
                EdgeData::Violates | EdgeData::EvaluatedAs => {
                    let source_idx = edge.source();
                    match self.graph.node_weight(source_idx) {
                        Some(NodeData::Runtime {
                            name,
                            version,
                            executable_path,
                        }) => {
                            machine_state = Some(format!(
                                "{} {} ({})",
                                name,
                                version,
                                executable_path.display()
                            ));
                            for out_edge in
                                self.graph.edges_directed(source_idx, Direction::Outgoing)
                            {
                                if *out_edge.weight() == EdgeData::SupportedBy {
                                    if let Some(NodeData::Evidence {
                                        description,
                                        confidence,
                                    }) = self.graph.node_weight(out_edge.target())
                                    {
                                        machine_evidence = Some(Evidence::new(
                                            concord_core::evidence::EvidenceSource::ExecutableInspection {
                                                path: executable_path.clone(),
                                                version_string: version.clone(),
                                                exit_code: 0,
                                            },
                                            *confidence,
                                            description.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                        Some(NodeData::Port { port, state }) => {
                            machine_state = Some(format!("port {} occupied ({:?})", port, state));
                            for out_edge in
                                self.graph.edges_directed(source_idx, Direction::Outgoing)
                            {
                                if *out_edge.weight() == EdgeData::SupportedBy {
                                    if let Some(NodeData::Evidence {
                                        description,
                                        confidence,
                                    }) = self.graph.node_weight(out_edge.target())
                                    {
                                        machine_evidence = Some(Evidence::new(
                                            concord_core::evidence::EvidenceSource::NetworkProbe {
                                                target: format!("localhost:{}", port),
                                                outcome: "OCCUPIED".to_string(),
                                            },
                                            *confidence,
                                            description.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                        Some(NodeData::Service {
                            name,
                            status,
                            version,
                        }) => {
                            machine_state = Some(format!(
                                "service {} ({:?}, version: {:?})",
                                name, status, version
                            ));
                            for out_edge in
                                self.graph.edges_directed(source_idx, Direction::Outgoing)
                            {
                                if *out_edge.weight() == EdgeData::SupportedBy {
                                    if let Some(NodeData::Evidence {
                                        description,
                                        confidence,
                                    }) = self.graph.node_weight(out_edge.target())
                                    {
                                        machine_evidence = Some(Evidence::new(
                                            concord_core::evidence::EvidenceSource::DirectObservation {
                                                detail: format!("service: {}", name),
                                            },
                                            *confidence,
                                            description.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                        Some(NodeData::PackageManager {
                            name,
                            version,
                            executable_path,
                        }) => {
                            let ver_str = version.as_deref().unwrap_or("unknown");
                            machine_state = Some(format!(
                                "package manager {} {} ({})",
                                name,
                                ver_str,
                                executable_path.display()
                            ));
                            for out_edge in
                                self.graph.edges_directed(source_idx, Direction::Outgoing)
                            {
                                if *out_edge.weight() == EdgeData::SupportedBy {
                                    if let Some(NodeData::Evidence {
                                        description,
                                        confidence,
                                    }) = self.graph.node_weight(out_edge.target())
                                    {
                                        machine_evidence = Some(Evidence::new(
                                            concord_core::evidence::EvidenceSource::ExecutableInspection {
                                                path: executable_path.clone(),
                                                version_string: ver_str.to_string(),
                                                exit_code: 0,
                                            },
                                            *confidence,
                                            description.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                        Some(NodeData::Tool {
                            name,
                            version,
                            executable_path,
                        }) => {
                            let ver_str = version.as_deref().unwrap_or("unknown");
                            machine_state = Some(format!(
                                "tool {} {} ({})",
                                name,
                                ver_str,
                                executable_path.display()
                            ));
                            for out_edge in
                                self.graph.edges_directed(source_idx, Direction::Outgoing)
                            {
                                if *out_edge.weight() == EdgeData::SupportedBy {
                                    if let Some(NodeData::Evidence {
                                        description,
                                        confidence,
                                    }) = self.graph.node_weight(out_edge.target())
                                    {
                                        machine_evidence = Some(Evidence::new(
                                            concord_core::evidence::EvidenceSource::ExecutableInspection {
                                                path: executable_path.clone(),
                                                version_string: ver_str.to_string(),
                                                exit_code: 0,
                                            },
                                            *confidence,
                                            description.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                        Some(NodeData::Container {
                            name,
                            image,
                            status,
                        }) => {
                            machine_state = Some(format!(
                                "container {} (image: {}, status: {})",
                                name, image, status
                            ));
                            for out_edge in
                                self.graph.edges_directed(source_idx, Direction::Outgoing)
                            {
                                if *out_edge.weight() == EdgeData::SupportedBy {
                                    if let Some(NodeData::Evidence {
                                        description,
                                        confidence,
                                    }) = self.graph.node_weight(out_edge.target())
                                    {
                                        machine_evidence = Some(Evidence::new(
                                            concord_core::evidence::EvidenceSource::DirectObservation {
                                                detail: format!("container: {} {}", name, image),
                                            },
                                            *confidence,
                                            description.clone(),
                                        ));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // 4. Synthesize root cause and causal steps from graph path
        let target_comp = affected_components
            .first()
            .cloned()
            .unwrap_or_else(|| "project".to_string());
        let (root_cause, causal_steps) = match &constraint {
            Constraint::RuntimeVersion {
                runtime,
                constraint,
            } => {
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("missing or unresolvable");
                (
                    format!("{}.version {}", runtime, constraint),
                    vec![
                        format!(
                            "Component '{}' requires {} {}",
                            target_comp, runtime, constraint
                        ),
                        format!("Host machine runtime: {}", actual),
                        format!(
                            "First violated invariant: {} version satisfies {}",
                            runtime, constraint
                        ),
                        format!("Impact: {} build or startup cannot proceed", target_comp),
                    ],
                )
            }
            Constraint::PackageManagerVersion { name, constraint } => {
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("missing or unresolvable");
                let c_str = constraint
                    .as_ref()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "*".to_string());
                (
                    format!("{}.version {}", name, c_str),
                    vec![
                        format!(
                            "Component '{}' requires package manager {} {}",
                            target_comp, name, c_str
                        ),
                        format!("Host package manager: {}", actual),
                        format!("First violated invariant: {} satisfies {}", name, c_str),
                        "Impact: dependency resolution or task execution cannot proceed"
                            .to_string(),
                    ],
                )
            }
            Constraint::ToolAvailable {
                name,
                kind,
                scope,
                constraint,
            } => {
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("missing or unresolvable");
                let c_str = constraint
                    .as_ref()
                    .map(|c| format!(" {}", c))
                    .unwrap_or_default();
                (
                    format!("{}.available{}", name, c_str),
                    vec![
                        format!(
                            "Component '{}' declares tool '{}' ({}, scope: {}){}",
                            target_comp, name, kind, scope, c_str
                        ),
                        format!("Host machine state: {}", actual),
                        format!("First violated invariant: tool '{}' is available", name),
                        format!("Impact: tasks requiring '{}' cannot run", name),
                    ],
                )
            }
            Constraint::CompilerAvailable {
                language,
                min_standard,
                constraint,
            } => {
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("compiler missing or incompatible");
                let std_str = min_standard
                    .as_ref()
                    .map(|s| format!(" standard {}", s))
                    .unwrap_or_default();
                let ver_str = constraint
                    .as_ref()
                    .map(|c| format!(" version {}", c))
                    .unwrap_or_default();
                (
                    format!("{}.compiler{}{}", language, std_str, ver_str),
                    vec![
                        format!(
                            "Component '{}' requires compiler for '{}'{}{}",
                            target_comp, language, std_str, ver_str
                        ),
                        format!("Host machine state: {}", actual),
                        format!(
                            "First violated invariant: compiler for '{}' is available",
                            language
                        ),
                        format!("Impact: compilation for '{}' cannot proceed", language),
                    ],
                )
            }
            Constraint::LanguagePackageAvailable {
                language,
                package,
                constraint,
                scope,
            } => {
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("package missing from runtime environment");
                let ver_str = constraint
                    .as_ref()
                    .map(|c| format!(" {}", c))
                    .unwrap_or_default();
                (
                    format!("{}:{}.available{}", language, package, ver_str),
                    vec![
                        format!(
                            "Component '{}' requires {} package '{}'{} (scope: {})",
                            target_comp, language, package, ver_str, scope
                        ),
                        format!("Host runtime state: {}", actual),
                        format!(
                            "First violated invariant: package '{}' is installed in {}",
                            package, language
                        ),
                        format!(
                            "Impact: build or execution depending on '{}' will fail",
                            package
                        ),
                    ],
                )
            }
            Constraint::SystemLibraryAvailable {
                name,
                header,
                constraint,
                scope,
            } => {
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("system library missing or not found in library path");
                let header_str = header
                    .as_ref()
                    .map(|h| format!(" with header '{}'", h))
                    .unwrap_or_default();
                let ver_str = constraint
                    .as_ref()
                    .map(|c| format!(" {}", c))
                    .unwrap_or_default();

                let is_version_incompatible = match &status {
                    ConstraintStatus::Violated {
                        root_cause_hint, ..
                    } => root_cause_hint.contains("version_incompatible"),
                    _ => false,
                };
                let is_unknown = matches!(&status, ConstraintStatus::Unknown { .. });

                if is_version_incompatible {
                    (
                        format!("syslib.{}.version_incompatible{}", name, ver_str),
                        vec![
                            format!(
                                "Component '{}' requires system library '{}'{}{} (scope: {})",
                                target_comp, name, header_str, ver_str, scope
                            ),
                            format!(
                                "System library constraint: '{}' must satisfy{}",
                                name, ver_str
                            ),
                            format!("Host system state: {}", actual),
                            format!(
                                "First violated invariant: system library '{}' version satisfies{}",
                                name, ver_str
                            ),
                            format!(
                                "Impact: native linking or build configuration for '{}' will fail due to incompatible library version",
                                name
                            ),
                        ],
                    )
                } else if is_unknown {
                    (
                        format!("syslib.{}.version_unknown{}", name, ver_str),
                        vec![
                            format!(
                                "Component '{}' requires system library '{}'{}{} (scope: {})",
                                target_comp, name, header_str, ver_str, scope
                            ),
                            format!(
                                "System library constraint: '{}' must satisfy{}",
                                name, ver_str
                            ),
                            format!("Host system state: {}", actual),
                            format!(
                                "Unverified invariant: system library '{}' version satisfies{}",
                                name, ver_str
                            ),
                            format!(
                                "Impact: native linking or build configuration for '{}' may fail if installed version is incompatible",
                                name
                            ),
                        ],
                    )
                } else {
                    (
                        format!("{}.library{}{}", name, header_str, ver_str),
                        vec![
                            format!(
                                "Component '{}' requires system library '{}'{}{} (scope: {})",
                                target_comp, name, header_str, ver_str, scope
                            ),
                            format!("Host system state: {}", actual),
                            format!(
                                "First violated invariant: system library '{}' is installed",
                                name
                            ),
                            format!(
                                "Impact: native linking or build configuration for '{}' will fail",
                                name
                            ),
                        ],
                    )
                }
            }
            Constraint::PortAvailable { port } => {
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("occupied by existing socket");
                (
                    format!("port:{}.free", port),
                    vec![
                        format!("Component '{}' binds to port {}", target_comp, port),
                        format!("Host network state: {}", actual),
                        format!(
                            "First violated invariant: port {} must be free for binding",
                            port
                        ),
                        format!(
                            "Impact: listener socket collision EADDRINUSE on port {}",
                            port
                        ),
                    ],
                )
            }
            Constraint::ServiceRunning {
                service,
                min_version,
            } => {
                let ver_str = min_version
                    .as_deref()
                    .map(|v| format!(" (>= {})", v))
                    .unwrap_or_default();
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("service stopped or inactive");
                (
                    format!("service.{}.running", service),
                    vec![
                        format!(
                            "Component '{}' depends on service '{}{}'",
                            target_comp, service, ver_str
                        ),
                        format!("Host machine daemon state: {}", actual),
                        format!(
                            "First violated invariant: service {} is active and listening",
                            service
                        ),
                        format!("Impact: connection attempts to {} will be refused", service),
                    ],
                )
            }
            Constraint::EnvVarSet { key, .. } => (
                format!("env.{}.present", key),
                vec![
                    format!(
                        "Component '{}' declares required environment variable '{}'",
                        target_comp, key
                    ),
                    format!("Host shell environment: variable '{}' is missing", key),
                    format!("First violated invariant: environment contains '{}'", key),
                    format!(
                        "Impact: application configuration initialization for '{}' will fail",
                        key
                    ),
                ],
            ),
            Constraint::ConflictDetected { target, details } => (
                format!("{}.configuration_conflict", target),
                vec![
                    format!("Project declares contradictory {} specifications", target),
                    format!("Conflict details: {}", details),
                    "First violated invariant: coherent runtime version across configuration files"
                        .to_string(),
                    "Impact: build tools will select conflicting runtime versions".to_string(),
                ],
            ),
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
                let mut steps = Vec::new();
                if !missing_env_files.is_empty() {
                    let missing_str = missing_env_files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    steps.push(format!("Compose file references {}", missing_str));
                    steps.push(format!("{} does not exist", missing_str));
                    for t in env_templates {
                        steps.push(format!(
                            "Configuration template found: {}",
                            t.template_path.display()
                        ));
                    }
                } else {
                    steps.push(format!(
                        "Docker Compose project defined in '{}'",
                        compose_file.display()
                    ));
                }
                for suggestion in bootstrap_suggestions {
                    steps.push(format!(
                        "Deterministic bootstrap action found: {}",
                        suggestion
                    ));
                }
                if !unresolved_vars.is_empty() {
                    steps.push(format!(
                        "Required variables cannot be resolved: {}",
                        unresolved_vars.join(", ")
                    ));
                } else {
                    steps.push("Required variables cannot be resolved".to_string());
                }
                steps.push("Compose project cannot be instantiated".to_string());
                if !directly_affected_services.is_empty() {
                    steps.push(format!(
                        "Directly affected services: {}",
                        directly_affected_services.join(", ")
                    ));
                }
                if !transitively_blocked_services.is_empty() {
                    steps.push(format!(
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
                    steps.push(format!(
                        "dependent services cannot be created: {}",
                        all_affected.join(", ")
                    ));
                }

                let root_cause = if !missing_env_files.is_empty() {
                    format!("missing.env_file:{}", missing_env_files[0].display())
                } else {
                    "unresolved Compose variables".to_string()
                };
                (root_cause, steps)
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
                (
                    format!("compose.{}.state", service_name),
                    vec![
                        format!(
                            "Compose service '{}'{} declared in '{}'",
                            service_name,
                            c_str,
                            compose_file.display()
                        ),
                        format!("Host container state: {}", actual_state),
                        format!(
                            "First violated invariant: service container must be in state '{}'",
                            expected_state
                        ),
                        format!(
                            "Impact: connections to service '{}' will fail",
                            service_name
                        ),
                    ],
                )
            }
            Constraint::AnyOf {
                capability,
                constraints,
                scope,
            } => {
                let actual = machine_state
                    .as_deref()
                    .unwrap_or("no provider available on host");
                let alts_desc = constraints
                    .iter()
                    .map(|c| format!("{}", c))
                    .collect::<Vec<_>>()
                    .join(" OR ");
                (
                    format!("capability.{}.unsatisfied", capability),
                    vec![
                        format!(
                            "Component '{}' requires capability '{}' (alternatives: [{}]) (scope: {})",
                            target_comp, capability, alts_desc, scope
                        ),
                        format!("Host machine state: {}", actual),
                        format!(
                            "First violated invariant: at least one provider for '{}' must be installed and satisfied",
                            capability
                        ),
                        format!(
                            "Impact: feature or build requiring '{}' cannot proceed without an alternative provider",
                            capability
                        ),
                    ],
                )
            }
            _ => (
                format!("{}", constraint),
                vec![
                    format!(
                        "Component '{}' has requirement: {}",
                        target_comp, constraint
                    ),
                    format!("First violated invariant: {}", constraint),
                ],
            ),
        };

        Some(CausalTrace {
            constraint,
            status,
            requirement: Some(target_comp),
            root_cause: Some(root_cause),
            affected_components,
            causal_steps,
            project_evidence,
            machine_state,
            machine_evidence,
        })
    }

    /// Retrieve all causal traces for all violations currently in the graph.
    pub fn all_causal_traces(&self) -> Vec<CausalTrace> {
        let violations = self.find_violations();
        violations
            .into_iter()
            .filter_map(|idx| self.trace_causal_chain(idx))
            .collect()
    }
}
