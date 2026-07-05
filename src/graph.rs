//! `ResourceGraph` (DAG) — dependency resolution + topological sort.

use crate::errors::{Result, TerraformError};
use crate::resource::ResourceDef;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

// Resource graph (DAG)
// ---------------------------------------------------------------------------

/// Directed acyclic graph of resource definitions.
#[derive(Debug, Clone)]
pub struct ResourceGraph {
    pub(crate) nodes: BTreeMap<String, ResourceDef>,
    /// adjacency: from -> set of to (i.e. "from" depends on "to").
    pub(crate) edges: BTreeMap<String, BTreeSet<String>>,
}

impl Default for ResourceGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceGraph {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
        }
    }

    /// Number of resources in the graph.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Add a resource definition. Returns error on duplicate id.
    ///
    /// # Errors
    ///
    /// `DuplicateResource` if the id already exists.
    pub fn add(&mut self, def: ResourceDef) -> Result<()> {
        if self.nodes.contains_key(&def.id) {
            return Err(TerraformError::DuplicateResource(def.id));
        }
        let id = def.id.clone();
        let deps: Vec<String> = def.depends_on.clone();
        self.nodes.insert(id.clone(), def);
        for dep in deps {
            self.edges.entry(id.clone()).or_default().insert(dep);
        }
        Ok(())
    }

    /// Remove a resource by id.
    ///
    /// # Errors
    ///
    /// `ResourceNotFound` if the id does not exist.
    pub fn remove(&mut self, id: &str) -> Result<ResourceDef> {
        let def = self
            .nodes
            .remove(id)
            .ok_or_else(|| TerraformError::ResourceNotFound(id.to_owned()))?;
        self.edges.remove(id);
        // Remove reverse edges
        for edges in self.edges.values_mut() {
            edges.remove(id);
        }
        Ok(def)
    }

    /// Get a resource definition by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ResourceDef> {
        self.nodes.get(id)
    }

    /// Get a mutable reference to a resource definition.
    #[must_use]
    pub fn get_mut(&mut self, id: &str) -> Option<&mut ResourceDef> {
        self.nodes.get_mut(id)
    }

    /// Return all resource ids.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.nodes.keys().map(String::as_str).collect()
    }

    /// Add an explicit dependency edge.
    ///
    /// # Errors
    ///
    /// `ResourceNotFound` if either resource does not exist.
    pub fn add_dependency(&mut self, from: &str, to: &str) -> Result<()> {
        if !self.nodes.contains_key(from) {
            return Err(TerraformError::ResourceNotFound(from.to_owned()));
        }
        if !self.nodes.contains_key(to) {
            return Err(TerraformError::DependencyNotFound {
                from: from.to_owned(),
                to: to.to_owned(),
            });
        }
        self.edges
            .entry(from.to_owned())
            .or_default()
            .insert(to.to_owned());
        Ok(())
    }

    /// Return the direct dependencies of a resource.
    #[must_use]
    pub fn dependencies(&self, id: &str) -> Vec<&str> {
        self.edges
            .get(id)
            .map(|s| s.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Return all resources that depend on `id`.
    #[must_use]
    pub fn dependents(&self, id: &str) -> Vec<&str> {
        self.edges
            .iter()
            .filter_map(|(from, deps)| {
                if deps.contains(id) {
                    Some(from.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Validate that all dependency targets exist.
    ///
    /// # Errors
    ///
    /// `DependencyNotFound` if a target is missing.
    pub fn validate_dependencies(&self) -> Result<()> {
        for (from, deps) in &self.edges {
            for to in deps {
                if !self.nodes.contains_key(to) {
                    return Err(TerraformError::DependencyNotFound {
                        from: from.clone(),
                        to: to.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Topological sort using Kahn's algorithm. Returns resource ids in
    /// dependency order (dependencies first).
    ///
    /// # Errors
    ///
    /// `CycleDetected` if the graph contains a cycle.
    pub fn topological_sort(&self) -> Result<Vec<String>> {
        // in-degree map
        let mut in_degree: BTreeMap<&str, usize> = BTreeMap::new();
        for id in self.nodes.keys() {
            in_degree.entry(id).or_insert(0);
        }
        for deps in self.edges.values() {
            for dep in deps {
                if let Some(d) = in_degree.get_mut(dep.as_str()) {
                    // dep is depended-upon; but edges mean "from depends on to",
                    // so for topological sort we want edges from dependency to dependent.
                    // Actually we need to reverse: edge (from -> to) means from depends on to,
                    // so the execution order edge is to -> from.
                    // We'll recompute.
                    let _ = d;
                }
            }
        }

        // Reverse adjacency: execution graph edge: to -> from (to must run before from).
        let mut exec_adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let mut in_deg: BTreeMap<&str, usize> = BTreeMap::new();
        for id in self.nodes.keys() {
            in_deg.entry(id.as_str()).or_insert(0);
            exec_adj.entry(id.as_str()).or_default();
        }
        for (from, deps) in &self.edges {
            for to in deps {
                if self.nodes.contains_key(to) {
                    exec_adj.entry(to.as_str()).or_default().push(from.as_str());
                    *in_deg.entry(from.as_str()).or_insert(0) += 1;
                }
            }
        }

        let mut queue: VecDeque<&str> = VecDeque::new();
        for (&id, &deg) in &in_deg {
            if deg == 0 {
                queue.push_back(id);
            }
        }

        let mut order: Vec<String> = Vec::new();
        while let Some(id) = queue.pop_front() {
            order.push(id.to_owned());
            if let Some(neighbors) = exec_adj.get(id) {
                for &nb in neighbors {
                    if let Some(d) = in_deg.get_mut(nb) {
                        *d -= 1;
                        if *d == 0 {
                            queue.push_back(nb);
                        }
                    }
                }
            }
        }

        if order.len() != self.nodes.len() {
            return Err(TerraformError::CycleDetected);
        }

        Ok(order)
    }

    /// Detect whether the graph has a cycle.
    #[must_use]
    pub fn has_cycle(&self) -> bool {
        self.topological_sort().is_err()
    }
}
