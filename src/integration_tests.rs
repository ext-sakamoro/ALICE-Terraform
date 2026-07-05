//! Integration tests spanning multiple modules.

#![allow(
    clippy::float_cmp,
    clippy::unreadable_literal,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::too_many_lines,
    clippy::needless_range_loop,
    clippy::explicit_iter_loop,
    clippy::bool_to_int_with_if,
    clippy::approx_constant,
    clippy::cast_lossless,
    clippy::redundant_clone,
    clippy::format_collect,
    clippy::similar_names,
    clippy::needless_collect
)]

use crate::engine::*;
use crate::errors::*;
use crate::graph::*;
use crate::in_memory::*;
use crate::interpolation::*;
use crate::output::*;
use crate::plan::*;
use crate::provider::*;
use crate::resource::*;
use crate::state::*;
use crate::value::*;
use std::collections::{BTreeMap, HashMap};

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
    g.add(ResourceDef::new("db", "rds", "aws").property("size", Value::String("small".to_owned())))
        .unwrap();
    let r1 = engine.apply(&g).unwrap();
    assert_eq!(r1.created, vec!["db"]);

    // Update
    let mut g2 = ResourceGraph::new();
    g2.add(
        ResourceDef::new("db", "rds", "aws").property("size", Value::String("large".to_owned())),
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
        ResourceDef::new("existing", "t", "p").property("v", Value::String("changed".to_owned())),
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
