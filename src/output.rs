//! Output resolver (`resolve_output`).

use crate::errors::{Result, TerraformError};
use crate::state::State;
use crate::value::Value;

// Output resolver
// ---------------------------------------------------------------------------

/// Resolve output references like `${output.resource_id.key}`.
///
/// # Errors
///
/// `ResourceNotFound` or `VariableNotFound` for missing references.
pub fn resolve_output(state: &State, reference: &str) -> Result<Value> {
    let trimmed = reference
        .strip_prefix("${output.")
        .and_then(|s| s.strip_suffix('}'))
        .ok_or_else(|| {
            TerraformError::InterpolationError(format!("invalid output reference: {reference}"))
        })?;

    let (resource_id, key) = trimmed.split_once('.').ok_or_else(|| {
        TerraformError::InterpolationError(format!("invalid output reference: {reference}"))
    })?;

    let rs = state
        .get(resource_id)
        .ok_or_else(|| TerraformError::ResourceNotFound(resource_id.to_owned()))?;

    rs.outputs
        .get(key)
        .cloned()
        .ok_or_else(|| TerraformError::VariableNotFound(format!("{resource_id}.{key}")))
}
