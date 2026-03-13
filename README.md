**English** | [日本語](README_JP.md)

# ALICE-Terraform

Infrastructure as Code engine for the ALICE ecosystem. Provides resource graph management, state tracking, diff/plan, apply/destroy lifecycle, provider abstraction, and variable interpolation -- all in pure Rust.

## Features

- **Resource Graph** -- DAG-based dependency management with topological sort and cycle detection
- **State Management** -- Serializable state with resource properties and outputs, put/get/remove operations
- **Plan & Diff** -- Automatic diff between desired graph and current state producing create/update/destroy plans
- **Apply & Destroy** -- Execute plans through provider abstraction with result tracking
- **Provider Trait** -- Pluggable provider interface (create/update/destroy/read) with in-memory reference implementation
- **Variable Interpolation** -- `${var.name}` syntax for dynamic property values
- **Resource Import** -- Import existing resources into managed state
- **Output Resolution** -- Cross-resource output references via `resource_id.output_key` syntax

## Architecture

```
ResourceDef (type, properties, dependencies, outputs)
    |
    v
ResourceGraph (DAG, topological sort, cycle detection)
    |
    v
Plan::diff(graph, state) --> PlannedChange (Create/Update/Destroy)
    |
    v
Engine
    +-- register_provider(Provider trait)
    +-- apply(graph) --> ApplyResult
    +-- destroy() --> removed resource list
    +-- import(id, type, properties)
    |
    v
State (serializable resource state + outputs)
    |
    v
interpolate() --> variable substitution in properties
```

## Quick Start

```rust
use alice_terraform::*;

let mut graph = ResourceGraph::default();
graph.add(ResourceDef::new("web", "server")
    .property("size", Value::String("large".into())))?;

let mut engine = Engine::new(State::new());
engine.register_provider(&InMemoryProvider::new("server"));

let result = engine.apply(&graph)?;
```

## License

AGPL-3.0
