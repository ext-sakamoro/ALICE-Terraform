//! `InMemoryProvider` — in-memory provider (testing / demo).

use crate::errors::Result;
use crate::provider::{Provider, ProviderResult};
use crate::value::Properties;
use std::collections::{BTreeMap, HashMap};

// In-memory provider (for testing / demo)
// ---------------------------------------------------------------------------

/// A simple in-memory provider that tracks resources in a `HashMap`.
/// Useful for testing and demonstrations.
#[derive(Debug, Default)]
pub struct InMemoryProvider {
    name: String,
    resources: std::cell::RefCell<HashMap<String, Properties>>,
}

impl InMemoryProvider {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            resources: std::cell::RefCell::new(HashMap::new()),
        }
    }

    /// Number of resources tracked.
    #[must_use]
    pub fn resource_count(&self) -> usize {
        self.resources.borrow().len()
    }

    /// Check if a resource key exists.
    #[must_use]
    pub fn has_resource(&self, key: &str) -> bool {
        self.resources.borrow().contains_key(key)
    }
}

impl Provider for InMemoryProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn create(&self, resource_type: &str, properties: &Properties) -> Result<ProviderResult> {
        let key = format!("{}:{}", resource_type, properties.len());
        self.resources.borrow_mut().insert(key, properties.clone());
        Ok(ProviderResult {
            properties: properties.clone(),
            outputs: BTreeMap::new(),
        })
    }

    fn update(
        &self,
        resource_type: &str,
        _old_properties: &Properties,
        new_properties: &Properties,
    ) -> Result<ProviderResult> {
        let key = format!("{}:{}", resource_type, new_properties.len());
        self.resources
            .borrow_mut()
            .insert(key, new_properties.clone());
        Ok(ProviderResult {
            properties: new_properties.clone(),
            outputs: BTreeMap::new(),
        })
    }

    fn delete(&self, resource_type: &str, properties: &Properties) -> Result<()> {
        let key = format!("{}:{}", resource_type, properties.len());
        self.resources.borrow_mut().remove(&key);
        Ok(())
    }

    fn read(&self, resource_type: &str, import_id: &str) -> Result<ProviderResult> {
        let key = format!("{resource_type}:{import_id}");
        let store = self.resources.borrow();
        let properties = store.get(&key).map_or_else(Properties::new, Clone::clone);
        Ok(ProviderResult {
            properties,
            outputs: BTreeMap::new(),
        })
    }

    fn validate(&self, _resource_type: &str, _properties: &Properties) -> Result<()> {
        Ok(())
    }
}
