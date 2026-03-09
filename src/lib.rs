#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(clippy::module_name_repetitions)]

//! ALICE-Terraform: Infrastructure as Code engine.
//!
//! Provides resource graph (DAG), state management, diff/plan,
//! apply/destroy, provider abstraction, dependency resolution,
//! output values, variable interpolation, and import of existing resources.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;
use std::fmt::Write as _;

// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// Core value types
// ---------------------------------------------------------------------------

/// A property value that can be attached to a resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Value {
    String(String),
    Int(i64),
    Bool(bool),
    List(Vec<Self>),
    Null,
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::String(s) => write!(f, "{s}"),
            Self::Int(n) => write!(f, "{n}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::List(v) => {
                write!(f, "[")?;
                for (i, val) in v.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{val}")?;
                }
                write!(f, "]")
            }
            Self::Null => write!(f, "null"),
        }
    }
}

impl Value {
    /// Return the value as a string reference if it is a `String` variant.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        if let Self::String(s) = self {
            Some(s)
        } else {
            None
        }
    }

    /// Return the value as an `i64` if it is an `Int` variant.
    #[must_use]
    pub const fn as_int(&self) -> Option<i64> {
        if let Self::Int(n) = self {
            Some(*n)
        } else {
            None
        }
    }

    /// Return the value as a `bool` if it is a `Bool` variant.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        if let Self::Bool(b) = self {
            Some(*b)
        } else {
            None
        }
    }
}

/// Convenience alias for resource properties.
pub type Properties = BTreeMap<String, Value>;

// ---------------------------------------------------------------------------
// Variable interpolation
// ---------------------------------------------------------------------------

/// Interpolate `${var.NAME}` patterns inside a string value using the
/// provided variable map.
///
/// # Errors
///
/// Returns `InterpolationError` for unclosed `${` and `VariableNotFound`
/// when a referenced variable is missing.
pub fn interpolate<S: std::hash::BuildHasher>(
    input: &str,
    vars: &HashMap<String, Value, S>,
) -> Result<String> {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' && chars.peek() == Some(&'{') {
            chars.next(); // consume '{'
            let mut key = String::new();
            let mut closed = false;
            for c in chars.by_ref() {
                if c == '}' {
                    closed = true;
                    break;
                }
                key.push(c);
            }
            if !closed {
                return Err(TerraformError::InterpolationError("unclosed ${".to_owned()));
            }
            // Strip optional "var." prefix
            let var_name = key.strip_prefix("var.").unwrap_or(&key);
            let val = vars
                .get(var_name)
                .ok_or_else(|| TerraformError::VariableNotFound(var_name.to_owned()))?;
            result.push_str(&val.to_string());
        } else {
            result.push(ch);
        }
    }
    Ok(result)
}

/// Interpolate all `String` values inside a `Properties` map.
///
/// # Errors
///
/// Propagates interpolation errors.
pub fn interpolate_properties<S: std::hash::BuildHasher>(
    props: &Properties,
    vars: &HashMap<String, Value, S>,
) -> Result<Properties> {
    let mut out = Properties::new();
    for (k, v) in props {
        let new_v = match v {
            Value::String(s) => Value::String(interpolate(s, vars)?),
            other => other.clone(),
        };
        out.insert(k.clone(), new_v);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// Resource graph (DAG)
// ---------------------------------------------------------------------------

/// Directed acyclic graph of resource definitions.
#[derive(Debug, Clone)]
pub struct ResourceGraph {
    nodes: BTreeMap<String, ResourceDef>,
    /// adjacency: from -> set of to (i.e. "from" depends on "to").
    edges: BTreeMap<String, BTreeSet<String>>,
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

// ---------------------------------------------------------------------------
// State management
// ---------------------------------------------------------------------------

/// Recorded state of a single resource instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceState {
    pub id: String,
    pub resource_type: String,
    pub provider: String,
    pub properties: Properties,
    pub outputs: BTreeMap<String, Value>,
}

/// The full state of all managed resources.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub resources: BTreeMap<String, ResourceState>,
    pub serial: u64,
}

impl State {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a resource in the state.
    pub fn put(&mut self, rs: ResourceState) {
        self.resources.insert(rs.id.clone(), rs);
    }

    /// Remove a resource from the state.
    pub fn remove(&mut self, id: &str) -> Option<ResourceState> {
        self.resources.remove(id)
    }

    /// Get a resource from the state.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&ResourceState> {
        self.resources.get(id)
    }

    /// List all resource ids in state.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.resources.keys().map(String::as_str).collect()
    }

    /// Bump the serial number.
    pub const fn bump_serial(&mut self) {
        self.serial += 1;
    }

    /// Serialize state to a simple text format.
    #[must_use]
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "serial:{}", self.serial);
        for (id, rs) in &self.resources {
            let _ = writeln!(out, "resource:{}:{}:{}", id, rs.resource_type, rs.provider);
            for (k, v) in &rs.properties {
                let _ = writeln!(out, "  prop:{}:{}", k, serialize_value(v));
            }
            for (k, v) in &rs.outputs {
                let _ = writeln!(out, "  output:{}:{}", k, serialize_value(v));
            }
        }
        out
    }

    /// Deserialize state from the text format.
    ///
    /// # Errors
    ///
    /// `SerdeError` on malformed input.
    pub fn deserialize(input: &str) -> Result<Self> {
        let mut state = Self::new();
        let mut current: Option<ResourceState> = None;

        for line in input.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(serial_str) = trimmed.strip_prefix("serial:") {
                state.serial = serial_str
                    .parse()
                    .map_err(|e| TerraformError::SerdeError(format!("invalid serial: {e}")))?;
            } else if let Some(rest) = trimmed.strip_prefix("resource:") {
                // Flush previous resource
                if let Some(rs) = current.take() {
                    state.put(rs);
                }
                let parts: Vec<&str> = rest.splitn(3, ':').collect();
                if parts.len() < 3 {
                    return Err(TerraformError::SerdeError(
                        "invalid resource line".to_owned(),
                    ));
                }
                current = Some(ResourceState {
                    id: parts[0].to_owned(),
                    resource_type: parts[1].to_owned(),
                    provider: parts[2].to_owned(),
                    properties: Properties::new(),
                    outputs: BTreeMap::new(),
                });
            } else if let Some(rest) = trimmed.strip_prefix("prop:") {
                let (key, val) = parse_kv(rest)?;
                if let Some(ref mut rs) = current {
                    rs.properties.insert(key, val);
                }
            } else if let Some(rest) = trimmed.strip_prefix("output:") {
                let (key, val) = parse_kv(rest)?;
                if let Some(ref mut rs) = current {
                    rs.outputs.insert(key, val);
                }
            }
        }

        if let Some(rs) = current {
            state.put(rs);
        }
        Ok(state)
    }

    /// Number of resources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.resources.len()
    }

    /// Whether state is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.resources.is_empty()
    }
}

fn serialize_value(v: &Value) -> String {
    match v {
        Value::String(s) => format!("s:{s}"),
        Value::Int(n) => format!("i:{n}"),
        Value::Bool(b) => format!("b:{b}"),
        Value::Null => "n:".to_owned(),
        Value::List(items) => {
            let inner: Vec<String> = items.iter().map(serialize_value).collect();
            format!("l:{}", inner.join(";"))
        }
    }
}

fn parse_kv(input: &str) -> Result<(String, Value)> {
    let (key, val_str) = input
        .split_once(':')
        .ok_or_else(|| TerraformError::SerdeError("missing colon in kv".to_owned()))?;
    let val = deserialize_value(val_str)?;
    Ok((key.to_owned(), val))
}

fn deserialize_value(input: &str) -> Result<Value> {
    if let Some(rest) = input.strip_prefix("s:") {
        Ok(Value::String(rest.to_owned()))
    } else if let Some(rest) = input.strip_prefix("i:") {
        let n: i64 = rest
            .parse()
            .map_err(|e| TerraformError::SerdeError(format!("invalid int: {e}")))?;
        Ok(Value::Int(n))
    } else if let Some(rest) = input.strip_prefix("b:") {
        let b: bool = rest
            .parse()
            .map_err(|e| TerraformError::SerdeError(format!("invalid bool: {e}")))?;
        Ok(Value::Bool(b))
    } else if input.starts_with("n:") {
        Ok(Value::Null)
    } else if let Some(rest) = input.strip_prefix("l:") {
        if rest.is_empty() {
            return Ok(Value::List(Vec::new()));
        }
        let items: Result<Vec<Value>> = rest.split(';').map(deserialize_value).collect();
        Ok(Value::List(items?))
    } else {
        Err(TerraformError::SerdeError(format!(
            "unknown value prefix: {input}"
        )))
    }
}

// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Value tests --

    #[test]
    fn value_string_display() {
        let v = Value::String("hello".to_owned());
        assert_eq!(v.to_string(), "hello");
    }

    #[test]
    fn value_int_display() {
        let v = Value::Int(42);
        assert_eq!(v.to_string(), "42");
    }

    #[test]
    fn value_bool_display() {
        assert_eq!(Value::Bool(true).to_string(), "true");
        assert_eq!(Value::Bool(false).to_string(), "false");
    }

    #[test]
    fn value_null_display() {
        assert_eq!(Value::Null.to_string(), "null");
    }

    #[test]
    fn value_list_display() {
        let v = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(v.to_string(), "[1, 2]");
    }

    #[test]
    fn value_as_str() {
        let v = Value::String("x".to_owned());
        assert_eq!(v.as_str(), Some("x"));
        assert_eq!(Value::Int(0).as_str(), None);
    }

    #[test]
    fn value_as_int() {
        assert_eq!(Value::Int(7).as_int(), Some(7));
        assert_eq!(Value::Null.as_int(), None);
    }

    #[test]
    fn value_as_bool() {
        assert_eq!(Value::Bool(true).as_bool(), Some(true));
        assert_eq!(Value::Null.as_bool(), None);
    }

    #[test]
    fn value_empty_list_display() {
        let v = Value::List(vec![]);
        assert_eq!(v.to_string(), "[]");
    }

    // -- Interpolation tests --

    #[test]
    fn interpolate_simple_var() {
        let mut vars = HashMap::new();
        vars.insert("region".to_owned(), Value::String("us-east-1".to_owned()));
        let result = interpolate("deploy to ${var.region}", &vars).unwrap();
        assert_eq!(result, "deploy to us-east-1");
    }

    #[test]
    fn interpolate_no_prefix() {
        let mut vars = HashMap::new();
        vars.insert("name".to_owned(), Value::String("alice".to_owned()));
        let result = interpolate("hello ${name}", &vars).unwrap();
        assert_eq!(result, "hello alice");
    }

    #[test]
    fn interpolate_multiple() {
        let mut vars = HashMap::new();
        vars.insert("a".to_owned(), Value::String("X".to_owned()));
        vars.insert("b".to_owned(), Value::Int(99));
        let result = interpolate("${a}-${b}", &vars).unwrap();
        assert_eq!(result, "X-99");
    }

    #[test]
    fn interpolate_missing_var() {
        let vars = HashMap::new();
        let result = interpolate("${var.missing}", &vars);
        assert!(matches!(result, Err(TerraformError::VariableNotFound(_))));
    }

    #[test]
    fn interpolate_unclosed() {
        let vars = HashMap::new();
        let result = interpolate("${unclosed", &vars);
        assert!(matches!(result, Err(TerraformError::InterpolationError(_))));
    }

    #[test]
    fn interpolate_no_vars() {
        let vars = HashMap::new();
        let result = interpolate("no vars here", &vars).unwrap();
        assert_eq!(result, "no vars here");
    }

    #[test]
    fn interpolate_empty_string() {
        let vars = HashMap::new();
        let result = interpolate("", &vars).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn interpolate_properties_mixed() {
        let mut vars = HashMap::new();
        vars.insert("env".to_owned(), Value::String("prod".to_owned()));
        let mut props = Properties::new();
        props.insert("name".to_owned(), Value::String("${var.env}-db".to_owned()));
        props.insert("count".to_owned(), Value::Int(3));
        let result = interpolate_properties(&props, &vars).unwrap();
        assert_eq!(
            result.get("name"),
            Some(&Value::String("prod-db".to_owned()))
        );
        assert_eq!(result.get("count"), Some(&Value::Int(3)));
    }

    #[test]
    fn interpolate_dollar_without_brace() {
        let vars = HashMap::new();
        let result = interpolate("price $5", &vars).unwrap();
        assert_eq!(result, "price $5");
    }

    // -- ResourceDef tests --

    #[test]
    fn resource_def_builder() {
        let def = ResourceDef::new("web", "instance", "aws")
            .property("size", Value::String("t3.micro".to_owned()))
            .depends("vpc")
            .output("ip", Value::String("1.2.3.4".to_owned()));
        assert_eq!(def.id, "web");
        assert_eq!(def.resource_type, "instance");
        assert_eq!(def.provider, "aws");
        assert_eq!(def.properties.len(), 1);
        assert_eq!(def.depends_on, vec!["vpc"]);
        assert_eq!(def.outputs.len(), 1);
    }

    // -- ResourceGraph tests --

    #[test]
    fn graph_add_and_get() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        assert_eq!(g.len(), 1);
        assert!(!g.is_empty());
        assert!(g.get("a").is_some());
    }

    #[test]
    fn graph_duplicate_error() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let err = g.add(ResourceDef::new("a", "t", "p")).unwrap_err();
        assert!(matches!(err, TerraformError::DuplicateResource(_)));
    }

    #[test]
    fn graph_remove() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let def = g.remove("a").unwrap();
        assert_eq!(def.id, "a");
        assert!(g.is_empty());
    }

    #[test]
    fn graph_remove_not_found() {
        let mut g = ResourceGraph::new();
        assert!(matches!(
            g.remove("x"),
            Err(TerraformError::ResourceNotFound(_))
        ));
    }

    #[test]
    fn graph_ids() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("b", "t", "p")).unwrap();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let ids = g.ids();
        assert_eq!(ids, vec!["a", "b"]); // BTreeMap = sorted
    }

    #[test]
    fn graph_default_is_empty() {
        let g = ResourceGraph::default();
        assert!(g.is_empty());
    }

    #[test]
    fn graph_get_mut() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let def = g.get_mut("a").unwrap();
        def.properties
            .insert("key".to_owned(), Value::String("val".to_owned()));
        assert_eq!(g.get("a").unwrap().properties.len(), 1);
    }

    #[test]
    fn graph_add_dependency() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        g.add(ResourceDef::new("b", "t", "p")).unwrap();
        g.add_dependency("b", "a").unwrap();
        assert_eq!(g.dependencies("b"), vec!["a"]);
    }

    #[test]
    fn graph_add_dependency_missing_from() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        assert!(matches!(
            g.add_dependency("x", "a"),
            Err(TerraformError::ResourceNotFound(_))
        ));
    }

    #[test]
    fn graph_add_dependency_missing_to() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        assert!(matches!(
            g.add_dependency("a", "x"),
            Err(TerraformError::DependencyNotFound { .. })
        ));
    }

    #[test]
    fn graph_dependents() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        g.add(ResourceDef::new("b", "t", "p").depends("a")).unwrap();
        assert_eq!(g.dependents("a"), vec!["b"]);
    }

    #[test]
    fn graph_validate_deps_ok() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        g.add(ResourceDef::new("b", "t", "p").depends("a")).unwrap();
        assert!(g.validate_dependencies().is_ok());
    }

    #[test]
    fn graph_validate_deps_missing() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        // Manually insert a bad edge
        g.edges
            .entry("a".to_owned())
            .or_default()
            .insert("missing".to_owned());
        assert!(matches!(
            g.validate_dependencies(),
            Err(TerraformError::DependencyNotFound { .. })
        ));
    }

    // -- Topological sort tests --

    #[test]
    fn topo_sort_linear() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("c", "t", "p").depends("b")).unwrap();
        g.add(ResourceDef::new("b", "t", "p").depends("a")).unwrap();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let order = g.topological_sort().unwrap();
        let pos_a = order.iter().position(|x| x == "a").unwrap();
        let pos_b = order.iter().position(|x| x == "b").unwrap();
        let pos_c = order.iter().position(|x| x == "c").unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn topo_sort_diamond() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        g.add(ResourceDef::new("b", "t", "p").depends("a")).unwrap();
        g.add(ResourceDef::new("c", "t", "p").depends("a")).unwrap();
        g.add(ResourceDef::new("d", "t", "p").depends("b").depends("c"))
            .unwrap();
        let order = g.topological_sort().unwrap();
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("a") < pos("b"));
        assert!(pos("a") < pos("c"));
        assert!(pos("b") < pos("d"));
        assert!(pos("c") < pos("d"));
    }

    #[test]
    fn topo_sort_cycle_detected() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p").depends("b")).unwrap();
        g.add(ResourceDef::new("b", "t", "p").depends("a")).unwrap();
        assert!(matches!(
            g.topological_sort(),
            Err(TerraformError::CycleDetected)
        ));
    }

    #[test]
    fn topo_sort_self_cycle() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        g.edges
            .entry("a".to_owned())
            .or_default()
            .insert("a".to_owned());
        assert!(g.has_cycle());
    }

    #[test]
    fn topo_sort_empty_graph() {
        let g = ResourceGraph::new();
        let order = g.topological_sort().unwrap();
        assert!(order.is_empty());
    }

    #[test]
    fn topo_sort_single_node() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("x", "t", "p")).unwrap();
        let order = g.topological_sort().unwrap();
        assert_eq!(order, vec!["x"]);
    }

    #[test]
    fn topo_sort_disconnected() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        g.add(ResourceDef::new("b", "t", "p")).unwrap();
        g.add(ResourceDef::new("c", "t", "p")).unwrap();
        let order = g.topological_sort().unwrap();
        assert_eq!(order.len(), 3);
    }

    #[test]
    fn has_cycle_false() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        g.add(ResourceDef::new("b", "t", "p").depends("a")).unwrap();
        assert!(!g.has_cycle());
    }

    // -- State tests --

    #[test]
    fn state_put_and_get() {
        let mut s = State::new();
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        assert_eq!(s.len(), 1);
        assert!(!s.is_empty());
        assert!(s.get("a").is_some());
    }

    #[test]
    fn state_remove() {
        let mut s = State::new();
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        let removed = s.remove("a");
        assert!(removed.is_some());
        assert!(s.is_empty());
    }

    #[test]
    fn state_ids() {
        let mut s = State::new();
        s.put(ResourceState {
            id: "b".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        assert_eq!(s.ids(), vec!["a", "b"]);
    }

    #[test]
    fn state_bump_serial() {
        let mut s = State::new();
        assert_eq!(s.serial, 0);
        s.bump_serial();
        assert_eq!(s.serial, 1);
    }

    #[test]
    fn state_serialize_deserialize_roundtrip() {
        let mut s = State::new();
        s.serial = 5;
        let mut props = Properties::new();
        props.insert("name".to_owned(), Value::String("web".to_owned()));
        props.insert("count".to_owned(), Value::Int(3));
        props.insert("enabled".to_owned(), Value::Bool(true));
        let mut outputs = BTreeMap::new();
        outputs.insert("ip".to_owned(), Value::String("1.2.3.4".to_owned()));
        s.put(ResourceState {
            id: "srv".to_owned(),
            resource_type: "instance".to_owned(),
            provider: "aws".to_owned(),
            properties: props,
            outputs,
        });
        let serialized = s.serialize();
        let deserialized = State::deserialize(&serialized).unwrap();
        assert_eq!(s, deserialized);
    }

    #[test]
    fn state_serialize_empty() {
        let s = State::new();
        let serialized = s.serialize();
        let deserialized = State::deserialize(&serialized).unwrap();
        assert_eq!(deserialized.serial, 0);
        assert!(deserialized.is_empty());
    }

    #[test]
    fn state_serialize_null_value() {
        let mut s = State::new();
        let mut props = Properties::new();
        props.insert("x".to_owned(), Value::Null);
        s.put(ResourceState {
            id: "r".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: props,
            outputs: BTreeMap::new(),
        });
        let serialized = s.serialize();
        let deserialized = State::deserialize(&serialized).unwrap();
        assert_eq!(
            deserialized.get("r").unwrap().properties.get("x"),
            Some(&Value::Null)
        );
    }

    #[test]
    fn state_serialize_list_value() {
        let mut s = State::new();
        let mut props = Properties::new();
        props.insert(
            "tags".to_owned(),
            Value::List(vec![
                Value::String("a".to_owned()),
                Value::String("b".to_owned()),
            ]),
        );
        s.put(ResourceState {
            id: "r".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: props,
            outputs: BTreeMap::new(),
        });
        let serialized = s.serialize();
        let deserialized = State::deserialize(&serialized).unwrap();
        assert_eq!(s, deserialized);
    }

    #[test]
    fn state_deserialize_invalid_serial() {
        let input = "serial:abc\n";
        assert!(matches!(
            State::deserialize(input),
            Err(TerraformError::SerdeError(_))
        ));
    }

    #[test]
    fn state_deserialize_invalid_resource_line() {
        let input = "serial:0\nresource:only_one_part\n";
        assert!(matches!(
            State::deserialize(input),
            Err(TerraformError::SerdeError(_))
        ));
    }

    #[test]
    fn state_default_is_empty() {
        let s = State::default();
        assert!(s.is_empty());
        assert_eq!(s.serial, 0);
    }

    // -- Plan / Diff tests --

    #[test]
    fn plan_create_only() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let s = State::new();
        let plan = Plan::diff(&g, &s);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.changes[0].kind, ChangeKind::Create);
    }

    #[test]
    fn plan_delete_only() {
        let g = ResourceGraph::new();
        let mut s = State::new();
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        let plan = Plan::diff(&g, &s);
        assert_eq!(plan.len(), 1);
        assert_eq!(plan.changes[0].kind, ChangeKind::Delete);
    }

    #[test]
    fn plan_update() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p").property("k", Value::String("new".to_owned())))
            .unwrap();
        let mut s = State::new();
        let mut props = Properties::new();
        props.insert("k".to_owned(), Value::String("old".to_owned()));
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: props,
            outputs: BTreeMap::new(),
        });
        let plan = Plan::diff(&g, &s);
        assert_eq!(plan.changes[0].kind, ChangeKind::Update);
    }

    #[test]
    fn plan_no_op() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let mut s = State::new();
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        let plan = Plan::diff(&g, &s);
        assert_eq!(plan.changes[0].kind, ChangeKind::NoOp);
    }

    #[test]
    fn plan_summary() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p").property("k", Value::String("new".to_owned())))
            .unwrap();
        g.add(ResourceDef::new("b", "t", "p")).unwrap();
        g.add(ResourceDef::new("new_r", "t", "p")).unwrap();

        let mut s = State::new();
        let mut props = Properties::new();
        props.insert("k".to_owned(), Value::String("old".to_owned()));
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: props,
            outputs: BTreeMap::new(),
        });
        s.put(ResourceState {
            id: "b".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        s.put(ResourceState {
            id: "old_r".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });

        let plan = Plan::diff(&g, &s);
        let summary = plan.summary();
        assert_eq!(summary.creates, 1);
        assert_eq!(summary.updates, 1);
        assert_eq!(summary.deletes, 1);
        assert_eq!(summary.no_ops, 1);
    }

    #[test]
    fn plan_actionable() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let mut s = State::new();
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        let plan = Plan::diff(&g, &s);
        assert!(plan.actionable().is_empty());
    }

    #[test]
    fn plan_is_empty_when_no_changes() {
        let g = ResourceGraph::new();
        let s = State::new();
        let plan = Plan::diff(&g, &s);
        assert!(plan.is_empty());
    }

    // -- InMemoryProvider tests --

    #[test]
    fn in_memory_provider_create() {
        let p = InMemoryProvider::new("test");
        assert_eq!(p.name(), "test");
        let mut props = Properties::new();
        props.insert("a".to_owned(), Value::Int(1));
        let result = p.create("instance", &props).unwrap();
        assert_eq!(result.properties.get("a"), Some(&Value::Int(1)));
        assert_eq!(p.resource_count(), 1);
    }

    #[test]
    fn in_memory_provider_update() {
        let p = InMemoryProvider::new("test");
        let old = Properties::new();
        let mut new_props = Properties::new();
        new_props.insert("b".to_owned(), Value::Bool(true));
        let result = p.update("instance", &old, &new_props).unwrap();
        assert_eq!(result.properties.get("b"), Some(&Value::Bool(true)));
    }

    #[test]
    fn in_memory_provider_delete() {
        let p = InMemoryProvider::new("test");
        let mut props = Properties::new();
        props.insert("a".to_owned(), Value::Int(1));
        p.create("instance", &props).unwrap();
        p.delete("instance", &props).unwrap();
        assert_eq!(p.resource_count(), 0);
    }

    #[test]
    fn in_memory_provider_read() {
        let p = InMemoryProvider::new("test");
        let result = p.read("instance", "some-id").unwrap();
        assert!(result.properties.is_empty());
    }

    #[test]
    fn in_memory_provider_validate() {
        let p = InMemoryProvider::new("test");
        assert!(p.validate("anything", &Properties::new()).is_ok());
    }

    #[test]
    fn in_memory_provider_has_resource() {
        let p = InMemoryProvider::new("test");
        assert!(!p.has_resource("instance:1"));
        let mut props = Properties::new();
        props.insert("a".to_owned(), Value::Int(1));
        p.create("instance", &props).unwrap();
        assert!(p.has_resource("instance:1"));
    }

    // -- Engine tests --

    #[test]
    fn engine_apply_creates() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p").property("k", Value::String("v".to_owned())))
            .unwrap();

        let result = engine.apply(&g).unwrap();
        assert_eq!(result.created, vec!["a"]);
        assert_eq!(result.total(), 1);
        assert!(engine.state().get("a").is_some());
    }

    #[test]
    fn engine_apply_updates() {
        let provider = InMemoryProvider::new("p");
        let mut s = State::new();
        let mut old_props = Properties::new();
        old_props.insert("k".to_owned(), Value::String("old".to_owned()));
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: old_props,
            outputs: BTreeMap::new(),
        });

        let mut engine = Engine::new(s);
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p").property("k", Value::String("new".to_owned())))
            .unwrap();

        let result = engine.apply(&g).unwrap();
        assert_eq!(result.updated, vec!["a"]);
        assert_eq!(
            engine.state().get("a").unwrap().properties.get("k"),
            Some(&Value::String("new".to_owned()))
        );
    }

    #[test]
    fn engine_apply_deletes() {
        let provider = InMemoryProvider::new("p");
        let mut s = State::new();
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });

        let mut engine = Engine::new(s);
        engine.register_provider(&provider);

        let g = ResourceGraph::new();
        let result = engine.apply(&g).unwrap();
        assert_eq!(result.deleted, vec!["a"]);
        assert!(engine.state().is_empty());
    }

    #[test]
    fn engine_apply_with_dependencies() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("vpc", "vpc", "p")).unwrap();
        g.add(ResourceDef::new("subnet", "subnet", "p").depends("vpc"))
            .unwrap();
        g.add(ResourceDef::new("instance", "instance", "p").depends("subnet"))
            .unwrap();

        let result = engine.apply(&g).unwrap();
        assert_eq!(result.created.len(), 3);
        assert_eq!(engine.state().len(), 3);
    }

    #[test]
    fn engine_apply_missing_provider() {
        let mut engine = Engine::new(State::new());
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "missing_provider"))
            .unwrap();
        assert!(matches!(
            engine.apply(&g),
            Err(TerraformError::ProviderError(_))
        ));
    }

    #[test]
    fn engine_apply_cycle_error() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p").depends("b")).unwrap();
        g.add(ResourceDef::new("b", "t", "p").depends("a")).unwrap();
        assert!(matches!(
            engine.apply(&g),
            Err(TerraformError::CycleDetected)
        ));
    }

    #[test]
    fn engine_destroy() {
        let provider = InMemoryProvider::new("p");
        let mut s = State::new();
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        s.put(ResourceState {
            id: "b".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });

        let mut engine = Engine::new(s);
        engine.register_provider(&provider);

        let destroyed = engine.destroy().unwrap();
        assert_eq!(destroyed.len(), 2);
        assert!(engine.state().is_empty());
    }

    #[test]
    fn engine_import() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        engine
            .import("imported", "instance", "p", "ext-123")
            .unwrap();
        assert!(engine.state().get("imported").is_some());
        assert_eq!(engine.state().serial, 1);
    }

    #[test]
    fn engine_import_missing_provider() {
        let mut engine = Engine::new(State::new());
        assert!(matches!(
            engine.import("x", "t", "missing", "id"),
            Err(TerraformError::ImportError(_))
        ));
    }

    #[test]
    fn engine_plan() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let plan = engine.plan(&g);
        assert_eq!(plan.summary().creates, 1);
    }

    #[test]
    fn engine_output() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p").output("ip", Value::String("10.0.0.1".to_owned())))
            .unwrap();
        engine.apply(&g).unwrap();

        assert_eq!(
            engine.output("a", "ip"),
            Some(&Value::String("10.0.0.1".to_owned()))
        );
        assert!(engine.output("a", "missing").is_none());
        assert!(engine.output("missing", "ip").is_none());
    }

    #[test]
    fn engine_outputs_map() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(
            ResourceDef::new("a", "t", "p")
                .output("ip", Value::String("10.0.0.1".to_owned()))
                .output("port", Value::Int(8080)),
        )
        .unwrap();
        engine.apply(&g).unwrap();

        let outputs = engine.outputs("a").unwrap();
        assert_eq!(outputs.len(), 2);
    }

    #[test]
    fn engine_state_mut() {
        let mut engine = Engine::new(State::new());
        engine.state_mut().serial = 42;
        assert_eq!(engine.state().serial, 42);
    }

    #[test]
    fn engine_apply_noop_doesnt_change_state_props() {
        let provider = InMemoryProvider::new("p");
        let mut s = State::new();
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        let initial_serial = s.serial;

        let mut engine = Engine::new(s);
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        let result = engine.apply(&g).unwrap();
        assert!(result.created.is_empty());
        assert!(result.updated.is_empty());
        assert!(result.deleted.is_empty());
        assert_eq!(engine.state().serial, initial_serial + 1);
    }

    // -- Output resolver tests --

    #[test]
    fn resolve_output_ok() {
        let mut s = State::new();
        let mut outputs = BTreeMap::new();
        outputs.insert("ip".to_owned(), Value::String("1.2.3.4".to_owned()));
        s.put(ResourceState {
            id: "web".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs,
        });
        let val = resolve_output(&s, "${output.web.ip}").unwrap();
        assert_eq!(val, Value::String("1.2.3.4".to_owned()));
    }

    #[test]
    fn resolve_output_missing_resource() {
        let s = State::new();
        assert!(matches!(
            resolve_output(&s, "${output.missing.ip}"),
            Err(TerraformError::ResourceNotFound(_))
        ));
    }

    #[test]
    fn resolve_output_missing_key() {
        let mut s = State::new();
        s.put(ResourceState {
            id: "web".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });
        assert!(matches!(
            resolve_output(&s, "${output.web.nope}"),
            Err(TerraformError::VariableNotFound(_))
        ));
    }

    #[test]
    fn resolve_output_invalid_syntax() {
        let s = State::new();
        assert!(matches!(
            resolve_output(&s, "not_a_ref"),
            Err(TerraformError::InterpolationError(_))
        ));
    }

    // -- Error display tests --

    #[test]
    fn error_display_cycle() {
        let e = TerraformError::CycleDetected;
        assert_eq!(e.to_string(), "cycle detected in resource graph");
    }

    #[test]
    fn error_display_resource_not_found() {
        let e = TerraformError::ResourceNotFound("x".to_owned());
        assert_eq!(e.to_string(), "resource not found: x");
    }

    #[test]
    fn error_display_variable_not_found() {
        let e = TerraformError::VariableNotFound("v".to_owned());
        assert_eq!(e.to_string(), "variable not found: v");
    }

    #[test]
    fn error_display_provider() {
        let e = TerraformError::ProviderError("fail".to_owned());
        assert_eq!(e.to_string(), "provider error: fail");
    }

    #[test]
    fn error_display_duplicate() {
        let e = TerraformError::DuplicateResource("a".to_owned());
        assert_eq!(e.to_string(), "duplicate resource: a");
    }

    #[test]
    fn error_display_dep_not_found() {
        let e = TerraformError::DependencyNotFound {
            from: "a".to_owned(),
            to: "b".to_owned(),
        };
        assert_eq!(e.to_string(), "dependency not found: a -> b");
    }

    #[test]
    fn error_display_import() {
        let e = TerraformError::ImportError("bad".to_owned());
        assert_eq!(e.to_string(), "import error: bad");
    }

    #[test]
    fn error_display_serde() {
        let e = TerraformError::SerdeError("parse".to_owned());
        assert_eq!(e.to_string(), "serde error: parse");
    }

    #[test]
    fn error_display_interpolation() {
        let e = TerraformError::InterpolationError("syntax".to_owned());
        assert_eq!(e.to_string(), "interpolation error: syntax");
    }

    #[test]
    fn error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(TerraformError::CycleDetected);
        assert!(!e.to_string().is_empty());
    }

    // -- Complex scenario tests --

    #[test]
    fn full_lifecycle_create_update_destroy() {
        let provider = InMemoryProvider::new("aws");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        // Create
        let mut g = ResourceGraph::new();
        g.add(
            ResourceDef::new("db", "rds", "aws")
                .property("size", Value::String("small".to_owned())),
        )
        .unwrap();
        let r1 = engine.apply(&g).unwrap();
        assert_eq!(r1.created, vec!["db"]);

        // Update
        let mut g2 = ResourceGraph::new();
        g2.add(
            ResourceDef::new("db", "rds", "aws")
                .property("size", Value::String("large".to_owned())),
        )
        .unwrap();
        let r2 = engine.apply(&g2).unwrap();
        assert_eq!(r2.updated, vec!["db"]);

        // Destroy
        let destroyed = engine.destroy().unwrap();
        assert_eq!(destroyed.len(), 1);
        assert!(engine.state().is_empty());
    }

    #[test]
    fn multi_provider_apply() {
        let aws = InMemoryProvider::new("aws");
        let gcp = InMemoryProvider::new("gcp");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&aws);
        engine.register_provider(&gcp);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("vm1", "instance", "aws")).unwrap();
        g.add(ResourceDef::new("vm2", "instance", "gcp")).unwrap();
        let result = engine.apply(&g).unwrap();
        assert_eq!(result.created.len(), 2);
    }

    #[test]
    fn state_roundtrip_multiple_resources() {
        let mut s = State::new();
        s.serial = 10;
        for i in 0..5 {
            let mut props = Properties::new();
            props.insert("idx".to_owned(), Value::Int(i));
            s.put(ResourceState {
                id: format!("r{i}"),
                resource_type: "t".to_owned(),
                provider: "p".to_owned(),
                properties: props,
                outputs: BTreeMap::new(),
            });
        }
        let serialized = s.serialize();
        let deserialized = State::deserialize(&serialized).unwrap();
        assert_eq!(s, deserialized);
    }

    #[test]
    fn plan_mixed_changes() {
        let mut g = ResourceGraph::new();
        // will be created
        g.add(ResourceDef::new("new1", "t", "p")).unwrap();
        g.add(ResourceDef::new("new2", "t", "p")).unwrap();
        // will be updated
        g.add(
            ResourceDef::new("existing", "t", "p")
                .property("v", Value::String("changed".to_owned())),
        )
        .unwrap();

        let mut s = State::new();
        let mut props = Properties::new();
        props.insert("v".to_owned(), Value::String("original".to_owned()));
        s.put(ResourceState {
            id: "existing".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: props,
            outputs: BTreeMap::new(),
        });
        // will be deleted
        s.put(ResourceState {
            id: "gone".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: Properties::new(),
            outputs: BTreeMap::new(),
        });

        let plan = Plan::diff(&g, &s);
        let summary = plan.summary();
        assert_eq!(summary.creates, 2);
        assert_eq!(summary.updates, 1);
        assert_eq!(summary.deletes, 1);
    }

    #[test]
    fn graph_remove_cleans_reverse_edges() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        g.add(ResourceDef::new("b", "t", "p").depends("a")).unwrap();
        g.remove("a").unwrap();
        // b's dependency edge to a should be cleaned
        assert!(g.dependencies("b").is_empty());
    }

    #[test]
    fn engine_serial_increments_on_apply() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);

        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        engine.apply(&g).unwrap();
        assert_eq!(engine.state().serial, 1);
        engine.apply(&g).unwrap();
        assert_eq!(engine.state().serial, 2);
    }

    #[test]
    fn engine_serial_increments_on_destroy() {
        let provider = InMemoryProvider::new("p");
        let mut engine = Engine::new(State::new());
        engine.register_provider(&provider);
        engine.destroy().unwrap();
        assert_eq!(engine.state().serial, 1);
    }

    #[test]
    fn large_graph_topo_sort() {
        let mut g = ResourceGraph::new();
        // Chain of 50 resources
        g.add(ResourceDef::new("r0", "t", "p")).unwrap();
        for i in 1..50 {
            g.add(ResourceDef::new(format!("r{i}"), "t", "p").depends(format!("r{}", i - 1)))
                .unwrap();
        }
        let order = g.topological_sort().unwrap();
        assert_eq!(order.len(), 50);
        for i in 1..50 {
            let pos_prev = order
                .iter()
                .position(|x| *x == format!("r{}", i - 1))
                .unwrap();
            let pos_curr = order.iter().position(|x| *x == format!("r{i}")).unwrap();
            assert!(pos_prev < pos_curr);
        }
    }

    #[test]
    fn interpolate_bool_value() {
        let mut vars = HashMap::new();
        vars.insert("flag".to_owned(), Value::Bool(true));
        let result = interpolate("enabled: ${var.flag}", &vars).unwrap();
        assert_eq!(result, "enabled: true");
    }

    #[test]
    fn interpolate_null_value() {
        let mut vars = HashMap::new();
        vars.insert("x".to_owned(), Value::Null);
        let result = interpolate("val=${x}", &vars).unwrap();
        assert_eq!(result, "val=null");
    }

    #[test]
    fn planned_change_old_new_properties() {
        let change = PlannedChange {
            resource_id: "a".to_owned(),
            kind: ChangeKind::Create,
            old_properties: None,
            new_properties: Some(Properties::new()),
        };
        assert!(change.old_properties.is_none());
        assert!(change.new_properties.is_some());
    }

    #[test]
    fn apply_result_total() {
        let r = ApplyResult {
            created: vec!["a".to_owned()],
            updated: vec!["b".to_owned(), "c".to_owned()],
            deleted: vec!["d".to_owned()],
        };
        assert_eq!(r.total(), 4);
    }

    #[test]
    fn value_equality() {
        assert_eq!(Value::Int(1), Value::Int(1));
        assert_ne!(Value::Int(1), Value::Int(2));
        assert_eq!(Value::Null, Value::Null);
        assert_ne!(Value::Null, Value::Int(0));
    }

    #[test]
    fn value_clone() {
        let v = Value::List(vec![Value::String("a".to_owned())]);
        let v2 = v.clone();
        assert_eq!(v, v2);
    }

    #[test]
    fn resource_def_clone() {
        let def = ResourceDef::new("a", "t", "p")
            .property("k", Value::Int(1))
            .depends("b");
        let def2 = def.clone();
        assert_eq!(def, def2);
    }

    #[test]
    fn state_overwrite_resource() {
        let mut s = State::new();
        let mut p1 = Properties::new();
        p1.insert("v".to_owned(), Value::Int(1));
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: p1,
            outputs: BTreeMap::new(),
        });
        let mut p2 = Properties::new();
        p2.insert("v".to_owned(), Value::Int(2));
        s.put(ResourceState {
            id: "a".to_owned(),
            resource_type: "t".to_owned(),
            provider: "p".to_owned(),
            properties: p2,
            outputs: BTreeMap::new(),
        });
        assert_eq!(s.len(), 1);
        assert_eq!(
            s.get("a").unwrap().properties.get("v"),
            Some(&Value::Int(2))
        );
    }

    #[test]
    fn deserialize_empty_list() {
        let val = deserialize_value("l:").unwrap();
        assert_eq!(val, Value::List(Vec::new()));
    }

    #[test]
    fn graph_dependencies_empty() {
        let g = ResourceGraph::new();
        assert!(g.dependencies("nonexistent").is_empty());
    }

    #[test]
    fn graph_dependents_empty() {
        let mut g = ResourceGraph::new();
        g.add(ResourceDef::new("a", "t", "p")).unwrap();
        assert!(g.dependents("a").is_empty());
    }

    #[test]
    fn engine_destroy_empty() {
        let mut engine = Engine::new(State::new());
        let destroyed = engine.destroy().unwrap();
        assert!(destroyed.is_empty());
    }

    #[test]
    fn engine_outputs_none_for_missing() {
        let engine = Engine::new(State::new());
        assert!(engine.outputs("missing").is_none());
    }
}
