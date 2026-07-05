//! Core value types (`Value` / `Properties`).

use std::collections::BTreeMap;
use std::fmt;

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
