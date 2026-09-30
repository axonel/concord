use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConcordError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Project analysis failed: {0}")]
    ProjectAnalysis(String),

    #[error("Machine scan failed: {0}")]
    MachineScan(String),

    #[error("Constraint evaluation failed: {0}")]
    ConstraintEvaluation(String),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("Invalid version constraint '{constraint}': {reason}")]
    InvalidVersionConstraint { constraint: String, reason: String },

    #[error("Verification error: {0}")]
    Verification(String),
}

pub type Result<T> = std::result::Result<T, ConcordError>;
