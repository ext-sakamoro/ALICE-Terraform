//! `Plan` + diff (`ChangeKind` / `PlannedChange` / `PlanSummary`).

use crate::graph::ResourceGraph;
use crate::state::State;
use crate::value::Properties;
use std::collections::BTreeSet;

// Diff / Plan
// ---------------------------------------------------------------------------

/// The kind of change for a resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Create,
    Update,
    Delete,
    NoOp,
}

/// A single planned change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedChange {
    pub resource_id: String,
    pub kind: ChangeKind,
    pub old_properties: Option<Properties>,
    pub new_properties: Option<Properties>,
}

/// A complete execution plan.
#[derive(Debug, Clone)]
pub struct Plan {
    pub changes: Vec<PlannedChange>,
}

impl Plan {
    /// Create a plan by diffing desired graph against current state.
    #[must_use]
    pub fn diff(graph: &ResourceGraph, state: &State) -> Self {
        let mut changes = Vec::new();

        // Resources in desired graph
        let desired_ids: BTreeSet<&str> = graph.nodes.keys().map(String::as_str).collect();
        let current_ids: BTreeSet<&str> = state.resources.keys().map(String::as_str).collect();

        // Creates and updates
        for id in &desired_ids {
            let def = &graph.nodes[*id];
            if let Some(current) = state.resources.get(*id) {
                if current.properties == def.properties {
                    changes.push(PlannedChange {
                        resource_id: (*id).to_owned(),
                        kind: ChangeKind::NoOp,
                        old_properties: Some(current.properties.clone()),
                        new_properties: Some(def.properties.clone()),
                    });
                } else {
                    changes.push(PlannedChange {
                        resource_id: (*id).to_owned(),
                        kind: ChangeKind::Update,
                        old_properties: Some(current.properties.clone()),
                        new_properties: Some(def.properties.clone()),
                    });
                }
            } else {
                changes.push(PlannedChange {
                    resource_id: (*id).to_owned(),
                    kind: ChangeKind::Create,
                    old_properties: None,
                    new_properties: Some(def.properties.clone()),
                });
            }
        }

        // Deletes
        for id in &current_ids {
            if !desired_ids.contains(id) {
                let current = &state.resources[*id];
                changes.push(PlannedChange {
                    resource_id: (*id).to_owned(),
                    kind: ChangeKind::Delete,
                    old_properties: Some(current.properties.clone()),
                    new_properties: None,
                });
            }
        }

        Self { changes }
    }

    /// Number of changes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.changes.len()
    }

    /// Whether the plan is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Filter to only actionable changes (non-NoOp).
    #[must_use]
    pub fn actionable(&self) -> Vec<&PlannedChange> {
        self.changes
            .iter()
            .filter(|c| c.kind != ChangeKind::NoOp)
            .collect()
    }

    /// Count changes by kind.
    #[must_use]
    pub fn summary(&self) -> PlanSummary {
        let mut s = PlanSummary::default();
        for c in &self.changes {
            match c.kind {
                ChangeKind::Create => s.creates += 1,
                ChangeKind::Update => s.updates += 1,
                ChangeKind::Delete => s.deletes += 1,
                ChangeKind::NoOp => s.no_ops += 1,
            }
        }
        s
    }
}

/// Summary counts for a plan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlanSummary {
    pub creates: usize,
    pub updates: usize,
    pub deletes: usize,
    pub no_ops: usize,
}
