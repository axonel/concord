pub mod format;

use serde::Serialize;
use std::path::Path;
use concord_constraints::evaluator::evaluate_project;
use concord_constraints::model::EvaluatedConstraint;
use concord_core::ir::{EnvironmentModel, MachineCapability, ProjectManifest};
use concord_diagnosis::{diagnose_all, Diagnosis};
use concord_graph::EnvironmentGraph;
use concord_predictor::{predict_failures, Prediction};
use concord_project::analyze_project;
use concord_verifier::{verify_environment, VerificationReport};

#[derive(Serialize)]
pub struct ConcordReport {
    pub project: ProjectManifest,
    pub machine: MachineCapability,
    pub evaluated_constraints: Vec<EvaluatedConstraint>,
    pub predictions: Vec<Prediction>,
    pub diagnoses: Vec<Diagnosis>,
    pub verification: VerificationReport,
}

pub struct PipelineOutput {
    pub env_model: EnvironmentModel,
    pub evaluated_constraints: Vec<EvaluatedConstraint>,
    pub graph: EnvironmentGraph,
    pub predictions: Vec<Prediction>,
    pub diagnoses: Vec<Diagnosis>,
    pub verification: VerificationReport,
}

pub fn execute_pipeline(target_path: &Path) -> Result<PipelineOutput, concord_core::ConcordError> {
    let project = analyze_project(target_path)?;
    let machine = concord_scanner::scan_machine_for_project(Some(target_path));
    let evaluated_constraints = evaluate_project(&project, &machine);
    let env_model = EnvironmentModel::new(project, machine);
    let graph = EnvironmentGraph::build(&env_model, &evaluated_constraints);
    let predictions = predict_failures(&env_model, &evaluated_constraints);
    let traces = graph.all_causal_traces();
    let diagnoses = diagnose_all(&predictions, &traces);
    let verification = verify_environment(&env_model, &evaluated_constraints);

    Ok(PipelineOutput {
        env_model,
        evaluated_constraints,
        graph,
        predictions,
        diagnoses,
        verification,
    })
}
