//! Provider abstraction (`Provider` trait + `ProviderResult`).

use crate::errors::Result;
use crate::value::Properties;
use crate::value::Value;
use std::collections::BTreeMap;

// Provider abstraction
// ---------------------------------------------------------------------------

/// Result from a provider operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderResult {
    pub properties: Properties,
    pub outputs: BTreeMap<String, Value>,
}

/// Trait that providers must implement to manage resources.
pub trait Provider {
    /// Provider name.
    fn name(&self) -> &str;

    /// Create a resource. Returns the final properties and outputs.
    ///
    /// # Errors
    ///
    /// Provider-specific errors.
    fn create(&self, resource_type: &str, properties: &Properties) -> Result<ProviderResult>;

    /// Update a resource.
    ///
    /// # Errors
    ///
    /// Provider-specific errors.
    fn update(
        &self,
        resource_type: &str,
        old_properties: &Properties,
        new_properties: &Properties,
    ) -> Result<ProviderResult>;

    /// Delete a resource.
    ///
    /// # Errors
    ///
    /// Provider-specific errors.
    fn delete(&self, resource_type: &str, properties: &Properties) -> Result<()>;

    /// Read (import) a resource by type and id.
    ///
    /// # Errors
    ///
    /// Provider-specific errors.
    fn read(&self, resource_type: &str, import_id: &str) -> Result<ProviderResult>;

    /// Validate properties for a resource type.
    ///
    /// # Errors
    ///
    /// Provider-specific validation errors.
    fn validate(&self, resource_type: &str, properties: &Properties) -> Result<()>;
}
