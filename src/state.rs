//! `State` + `ResourceState` — persistent state management.

use crate::errors::{Result, TerraformError};
use crate::value::{Properties, Value};
use std::collections::BTreeMap;
use std::fmt::Write as _;

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

pub fn deserialize_value(input: &str) -> Result<Value> {
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
