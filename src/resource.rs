//! `ResourceDef` — declarative resource definition.

use crate::value::{Properties, Value};
use std::collections::BTreeMap;

// Resource definition
// ---------------------------------------------------------------------------

/// A desired resource declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceDef {
    /// Unique identifier, e.g. `"aws_instance.web"`.
    pub id: String,
    /// Resource type understood by a provider.
    pub resource_type: String,
    /// Provider name.
    pub provider: String,
    /// Desired properties.
    pub properties: Properties,
    /// Explicit dependency ids.
    pub depends_on: Vec<String>,
    /// Output values exported by this resource after apply.
    pub outputs: BTreeMap<String, Value>,
}

impl ResourceDef {
    /// Create a new `ResourceDef`.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        resource_type: impl Into<String>,
        provider: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            resource_type: resource_type.into(),
            provider: provider.into(),
            properties: Properties::new(),
            depends_on: Vec::new(),
            outputs: BTreeMap::new(),
        }
    }

    /// Set a property.
    #[must_use]
    pub fn property(mut self, key: impl Into<String>, value: Value) -> Self {
        self.properties.insert(key.into(), value);
        self
    }

    /// Add a dependency.
    #[must_use]
    pub fn depends(mut self, dep: impl Into<String>) -> Self {
        self.depends_on.push(dep.into());
        self
    }

    /// Set an output value.
    #[must_use]
    pub fn output(mut self, key: impl Into<String>, value: Value) -> Self {
        self.outputs.insert(key.into(), value);
        self
    }
}
