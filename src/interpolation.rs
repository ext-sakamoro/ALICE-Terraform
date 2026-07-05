//! Variable interpolation (`${var.foo}` / `${resource.type.name.field}`).

use crate::errors::{Result, TerraformError};
use crate::value::{Properties, Value};
use std::collections::HashMap;

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
