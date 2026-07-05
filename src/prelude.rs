//! Convenience re-export (= `use alice_terraform::prelude::*;`).

pub use crate::engine::{ApplyResult, Engine};
pub use crate::errors::{Result, TerraformError};
pub use crate::graph::ResourceGraph;
pub use crate::in_memory::InMemoryProvider;
pub use crate::interpolation::{interpolate, interpolate_properties};
pub use crate::output::resolve_output;
pub use crate::plan::{ChangeKind, Plan, PlanSummary, PlannedChange};
pub use crate::provider::{Provider, ProviderResult};
pub use crate::resource::ResourceDef;
pub use crate::state::{ResourceState, State};
pub use crate::value::{Properties, Value};
