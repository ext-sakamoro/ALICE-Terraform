//! `TerraformError` — IaC engine error type.

use std::fmt;

// Errors
// ---------------------------------------------------------------------------

/// All possible errors produced by the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerraformError {
    /// A cycle was detected in the dependency graph.
    CycleDetected,
    /// A referenced resource does not exist.
    ResourceNotFound(String),
    /// A referenced variable is not defined.
    VariableNotFound(String),
    /// A provider returned an error.
    ProviderError(String),
    /// A duplicate resource id was detected.
    DuplicateResource(String),
    /// A dependency target does not exist.
    DependencyNotFound { from: String, to: String },
    /// Import failed.
    ImportError(String),
    /// Serialization / deserialization error.
    SerdeError(String),
    /// Interpolation syntax error.
    InterpolationError(String),
}

impl fmt::Display for TerraformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CycleDetected => write!(f, "cycle detected in resource graph"),
            Self::ResourceNotFound(id) => write!(f, "resource not found: {id}"),
            Self::VariableNotFound(name) => write!(f, "variable not found: {name}"),
            Self::ProviderError(msg) => write!(f, "provider error: {msg}"),
            Self::DuplicateResource(id) => write!(f, "duplicate resource: {id}"),
            Self::DependencyNotFound { from, to } => {
                write!(f, "dependency not found: {from} -> {to}")
            }
            Self::ImportError(msg) => write!(f, "import error: {msg}"),
            Self::SerdeError(msg) => write!(f, "serde error: {msg}"),
            Self::InterpolationError(msg) => write!(f, "interpolation error: {msg}"),
        }
    }
}

impl std::error::Error for TerraformError {}

pub type Result<T> = std::result::Result<T, TerraformError>;
