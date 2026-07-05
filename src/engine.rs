//! `Engine` — apply / destroy / import + `ApplyResult`.

use crate::errors::{Result, TerraformError};
use crate::graph::ResourceGraph;
use crate::plan::{ChangeKind, Plan, PlannedChange};
use crate::provider::Provider;
use crate::state::{ResourceState, State};
use crate::value::Value;
use std::collections::{BTreeMap, HashMap};

// Engine: apply / destroy / import
// ---------------------------------------------------------------------------

/// The main engine that orchestrates plan execution.
pub struct Engine<'a> {
    providers: HashMap<String, &'a dyn Provider>,
    state: State,
}

impl<'a> Engine<'a> {
    /// Create a new engine with the given initial state.
    #[must_use]
    pub fn new(state: State) -> Self {
        Self {
            providers: HashMap::new(),
            state,
        }
    }

    /// Register a provider.
    pub fn register_provider(&mut self, provider: &'a dyn Provider) {
        self.providers.insert(provider.name().to_owned(), provider);
    }

    /// Get current state.
    #[must_use]
    pub const fn state(&self) -> &State {
        &self.state
    }

    /// Get mutable state.
    pub const fn state_mut(&mut self) -> &mut State {
        &mut self.state
    }

    /// Create a plan by diffing the desired graph against the current state.
    #[must_use]
    pub fn plan(&self, graph: &ResourceGraph) -> Plan {
        Plan::diff(graph, &self.state)
    }

    /// Apply a plan, executing changes through providers in dependency order.
    ///
    /// # Errors
    ///
    /// Provider errors or missing provider.
    pub fn apply(&mut self, graph: &ResourceGraph) -> Result<ApplyResult> {
        let order = graph.topological_sort()?;
        graph.validate_dependencies()?;

        let plan = Plan::diff(graph, &self.state);
        let mut result = ApplyResult::default();

        // Build a set of planned changes indexed by id
        let change_map: HashMap<&str, &PlannedChange> = plan
            .changes
            .iter()
            .map(|c| (c.resource_id.as_str(), c))
            .collect();

        // Process creates/updates in dependency order
        for id in &order {
            if let Some(change) = change_map.get(id.as_str()) {
                let def = graph
                    .get(id)
                    .ok_or_else(|| TerraformError::ResourceNotFound(id.clone()))?;
                let provider = self.providers.get(&def.provider).ok_or_else(|| {
                    TerraformError::ProviderError(format!("provider not found: {}", def.provider))
                })?;

                match change.kind {
                    ChangeKind::Create => {
                        let pr = provider.create(&def.resource_type, &def.properties)?;
                        self.state.put(ResourceState {
                            id: id.clone(),
                            resource_type: def.resource_type.clone(),
                            provider: def.provider.clone(),
                            properties: pr.properties,
                            outputs: merge_outputs(&def.outputs, &pr.outputs),
                        });
                        result.created.push(id.clone());
                    }
                    ChangeKind::Update => {
                        let old_props = self
                            .state
                            .get(id)
                            .map(|r| r.properties.clone())
                            .unwrap_or_default();
                        let pr =
                            provider.update(&def.resource_type, &old_props, &def.properties)?;
                        self.state.put(ResourceState {
                            id: id.clone(),
                            resource_type: def.resource_type.clone(),
                            provider: def.provider.clone(),
                            properties: pr.properties,
                            outputs: merge_outputs(&def.outputs, &pr.outputs),
                        });
                        result.updated.push(id.clone());
                    }
                    ChangeKind::NoOp | ChangeKind::Delete => {}
                }
            }
        }

        // Process deletes (reverse dependency order)
        let delete_ids: Vec<String> = plan
            .changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Delete)
            .map(|c| c.resource_id.clone())
            .collect();

        for id in delete_ids.iter().rev() {
            if let Some(rs) = self.state.get(id) {
                let provider_name = rs.provider.clone();
                let resource_type = rs.resource_type.clone();
                let properties = rs.properties.clone();
                let provider = self.providers.get(&provider_name).ok_or_else(|| {
                    TerraformError::ProviderError(format!("provider not found: {provider_name}"))
                })?;
                provider.delete(&resource_type, &properties)?;
                self.state.remove(id);
                result.deleted.push(id.clone());
            }
        }

        self.state.bump_serial();
        Ok(result)
    }

    /// Destroy all resources in the current state.
    ///
    /// # Errors
    ///
    /// Provider errors.
    pub fn destroy(&mut self) -> Result<Vec<String>> {
        let ids: Vec<String> = self.state.ids().iter().map(|s| (*s).to_owned()).collect();
        let mut destroyed = Vec::new();

        for id in ids.iter().rev() {
            if let Some(rs) = self.state.get(id) {
                let provider_name = rs.provider.clone();
                let resource_type = rs.resource_type.clone();
                let properties = rs.properties.clone();
                if let Some(provider) = self.providers.get(&provider_name) {
                    provider.delete(&resource_type, &properties)?;
                }
                self.state.remove(id);
                destroyed.push(id.clone());
            }
        }

        self.state.bump_serial();
        Ok(destroyed)
    }

    /// Import an existing resource into state.
    ///
    /// # Errors
    ///
    /// Provider or import errors.
    pub fn import(
        &mut self,
        id: &str,
        resource_type: &str,
        provider_name: &str,
        import_id: &str,
    ) -> Result<()> {
        let provider = self.providers.get(provider_name).ok_or_else(|| {
            TerraformError::ImportError(format!("provider not found: {provider_name}"))
        })?;
        let pr = provider.read(resource_type, import_id)?;
        self.state.put(ResourceState {
            id: id.to_owned(),
            resource_type: resource_type.to_owned(),
            provider: provider_name.to_owned(),
            properties: pr.properties,
            outputs: pr.outputs,
        });
        self.state.bump_serial();
        Ok(())
    }

    /// Get output value from a resource in state.
    #[must_use]
    pub fn output(&self, resource_id: &str, output_key: &str) -> Option<&Value> {
        self.state
            .get(resource_id)
            .and_then(|rs| rs.outputs.get(output_key))
    }

    /// Get all outputs for a resource.
    #[must_use]
    pub fn outputs(&self, resource_id: &str) -> Option<&BTreeMap<String, Value>> {
        self.state.get(resource_id).map(|rs| &rs.outputs)
    }
}

fn merge_outputs(
    def_outputs: &BTreeMap<String, Value>,
    provider_outputs: &BTreeMap<String, Value>,
) -> BTreeMap<String, Value> {
    let mut merged = def_outputs.clone();
    for (k, v) in provider_outputs {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// Result of an apply operation.
#[derive(Debug, Clone, Default)]
pub struct ApplyResult {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub deleted: Vec<String>,
}

impl ApplyResult {
    /// Total number of changes.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.created.len() + self.updated.len() + self.deleted.len()
    }
}
