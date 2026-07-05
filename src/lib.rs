//! ALICE-Terraform: Infrastructure as Code engine.
//!
//! Provides resource graph (DAG), state management, diff/plan,
//! apply/destroy, provider abstraction, dependency resolution,
//! output values, variable interpolation, and import of existing resources.

#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::wildcard_imports,
    clippy::doc_markdown,
    clippy::too_many_lines,
    clippy::similar_names,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else
)]

pub mod engine;
pub mod errors;
pub mod graph;
pub mod in_memory;
pub mod interpolation;
pub mod output;
pub mod plan;
pub mod prelude;
pub mod provider;
pub mod resource;
pub mod state;
pub mod value;

#[cfg(test)]
mod integration_tests;

// Backward-compat re-exports.
pub use crate::engine::*;
pub use crate::errors::*;
pub use crate::graph::*;
pub use crate::in_memory::*;
pub use crate::interpolation::*;
pub use crate::output::*;
pub use crate::plan::*;
pub use crate::provider::*;
pub use crate::resource::*;
pub use crate::state::*;
pub use crate::value::*;
