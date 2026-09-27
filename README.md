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

`AGPL-3.0 OR LicenseRef-Commercial` — dual-licensed. Pick either.

| Option | Terms | Use it when |
|--------|-------|-------------|
| **AGPL-3.0** | [LICENSE-AGPL](LICENSE-AGPL) — free, no reporting obligation | Your project is itself AGPL-compatible open source, or you are only using it internally |
| **Commercial License** | [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md) — paid, removes the copyleft | Closed-source product, proprietary SaaS, edge / firmware distribution, plugin redistribution, or a platform NDA that forbids source disclosure |

AGPL is a strong copyleft: a product, firmware image, or service that links
`alice-terraform` and is distributed or served to users must be released under the AGPL
as well. That is intentional for the open ecosystem, and the Commercial
License exists for the cases where it is not something you are able to do.

Commercial licence enquiries: <contact@extoria.co.jp>
