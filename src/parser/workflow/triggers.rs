use crate::ir::*;
use saphyr::{MarkedYaml, Scalar, YamlData};
use std::path::Path;

use super::helpers::{as_bool, as_str, get_field, str_list_from, stringify_value};

pub(super) fn parse_on(
    value: Option<&MarkedYaml<'_>>,
    path: &Path,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Vec<TriggerSpec> {
    let Some(value) = value else {
        return Vec::new();
    };
    match &value.data {
        YamlData::Value(Scalar::String(name)) => vec![bare_trigger(name)],
        YamlData::Sequence(items) => items
            .iter()
            .filter_map(|item| as_str(item).map(bare_trigger))
            .collect(),
        YamlData::Mapping(map) => map
            .iter()
            .map(|(k, v)| {
                let event_name = as_str(k).unwrap_or("");
                trigger_from_map_entry(event_name, v, path, diagnostics)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Construct a trigger from just the event name (no body — `on: push` or
/// inside an `on: [push, pull_request]` sequence). Events with payload
/// concepts (schedule / workflow_call / workflow_dispatch / workflow_run) get
/// an empty `EventExtras` via `TriggerSpec::bare`, so accessor methods like
/// `Workflow::inputs()` can distinguish "reusable workflow with no inputs
/// declared" from "not a reusable workflow at all".
fn bare_trigger(event_name: &str) -> TriggerSpec {
    TriggerSpec::bare(EventKind::from_name(event_name))
}

fn trigger_from_map_entry(
    event_name: &str,
    body: &MarkedYaml<'_>,
    path: &Path,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> TriggerSpec {
    let event = EventKind::from_name(event_name);

    if matches!(&body.data, YamlData::Value(Scalar::Null)) {
        return bare_trigger(event_name);
    }

    let branches = parse_ref_filter(body, "branches", "branches-ignore");
    let tags = parse_ref_filter(body, "tags", "tags-ignore");
    let paths = parse_ref_filter(body, "paths", "paths-ignore");
    let types = parse_types_field(get_field(body, "types"), &event, path, diagnostics);
    let extras = parse_extras(&event, body);

    TriggerSpec {
        event,
        branches,
        tags,
        paths,
        types,
        extras,
    }
}

/// Decide between Include / Exclude / None based on which key is present.
/// `branches` and `branches-ignore` are mutually exclusive per GitHub spec;
/// when both are present (invalid YAML), Include wins.
fn parse_ref_filter(body: &MarkedYaml<'_>, include_key: &str, exclude_key: &str) -> RefFilter {
    if let Some(node) = get_field(body, include_key) {
        return RefFilter::Include {
            patterns: str_list_from(node),
        };
    }
    if let Some(node) = get_field(body, exclude_key) {
        return RefFilter::Exclude {
            patterns: str_list_from(node),
        };
    }
    RefFilter::None
}

/// `None` when the key is absent. `Some(vec)` when present (including
/// `Some(vec![])` for an explicit `types: []`). The distinction matters for
/// `pull_request` default-subset semantics.
fn parse_types_field(
    value: Option<&MarkedYaml<'_>>,
    event: &EventKind,
    path: &Path,
    diagnostics: &mut Vec<ParseDiagnostic>,
) -> Option<Vec<String>> {
    let node = value?;
    let items = str_list_from(node);

    if let Some(allowed) = event.allowed_activity_types() {
        // Validate each item against the closed set. Emit a diagnostic for
        // every unknown value; store all values as-is so the IR is complete.
        match &node.data {
            YamlData::Sequence(seq) => {
                for (item_node, item_str) in seq.iter().zip(items.iter()) {
                    if !allowed.contains(&item_str.as_str()) {
                        let line = item_node.span.start.line();
                        diagnostics.push(ParseDiagnostic {
                            file: path.to_path_buf(),
                            line,
                            message: format!(
                                "unknown activity type `{item_str}` for event `{event_name}`; \
                                 valid types: {valid}",
                                event_name = event.name(),
                                valid = allowed.join(", "),
                            ),
                        });
                    }
                }
            }
            // scalar `types: opened` (single value, no sequence)
            YamlData::Value(Scalar::String(s)) if !allowed.contains(&s.as_ref()) => {
                let line = node.span.start.line();
                diagnostics.push(ParseDiagnostic {
                    file: path.to_path_buf(),
                    line,
                    message: format!(
                        "unknown activity type `{s}` for event `{event_name}`; \
                         valid types: {valid}",
                        event_name = event.name(),
                        valid = allowed.join(", "),
                    ),
                });
            }
            _ => {}
        }
    }

    Some(items)
}

fn parse_extras(event: &EventKind, body: &MarkedYaml<'_>) -> Option<EventExtras> {
    match event {
        EventKind::Schedule => Some(EventExtras::Schedule {
            entries: parse_schedule_entries(body),
        }),
        EventKind::WorkflowDispatch => Some(EventExtras::WorkflowDispatch {
            inputs: parse_input_decls(get_field(body, "inputs")),
        }),
        EventKind::WorkflowCall => Some(EventExtras::WorkflowCall {
            inputs: parse_input_decls(get_field(body, "inputs")),
            outputs: parse_output_decls(get_field(body, "outputs")),
            secrets: parse_secret_decls(get_field(body, "secrets")),
        }),
        EventKind::WorkflowRun => Some(EventExtras::WorkflowRun {
            workflows: get_field(body, "workflows")
                .map(str_list_from)
                .unwrap_or_default(),
        }),
        _ => None,
    }
}

fn parse_schedule_entries(body: &MarkedYaml<'_>) -> Vec<ScheduleEntry> {
    match &body.data {
        YamlData::Sequence(seq) => seq
            .iter()
            .filter_map(|item| {
                let cron = get_field(item, "cron").and_then(as_str)?.to_string();
                let timezone = get_field(item, "timezone")
                    .and_then(as_str)
                    .map(|s| s.to_string());
                Some(ScheduleEntry { cron, timezone })
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn parse_input_decls(value: Option<&MarkedYaml<'_>>) -> Vec<InputDecl> {
    let Some(node) = value else {
        return Vec::new();
    };
    let YamlData::Mapping(map) = &node.data else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(k, v)| {
            let name = as_str(k).filter(|s| !s.is_empty())?.to_string();
            Some(InputDecl {
                name,
                required: get_field(v, "required").and_then(as_bool).unwrap_or(false),
                default: parse_default_scalar(get_field(v, "default")),
                input_type: parse_input_type(get_field(v, "type"), get_field(v, "options")),
            })
        })
        .collect()
}

pub(super) fn parse_default_scalar(value: Option<&MarkedYaml<'_>>) -> Option<String> {
    let node = value?;
    match &node.data {
        YamlData::Value(Scalar::Null) => None,
        _ => Some(stringify_value(node)),
    }
}

fn parse_input_type(
    type_value: Option<&MarkedYaml<'_>>,
    options_value: Option<&MarkedYaml<'_>>,
) -> Option<InputType> {
    let raw = as_str(type_value?)?;
    match raw {
        "string" => Some(InputType::String),
        "boolean" => Some(InputType::Boolean),
        "number" => Some(InputType::Number),
        "choice" => {
            let options = match options_value.map(|n| &n.data) {
                Some(YamlData::Sequence(seq)) => seq
                    .iter()
                    .filter_map(|x| as_str(x).map(|s| s.to_string()))
                    .collect(),
                _ => Vec::new(),
            };
            Some(InputType::Choice { options })
        }
        "environment" => Some(InputType::Environment),
        _ => None,
    }
}

pub(crate) fn parse_output_decls(value: Option<&MarkedYaml<'_>>) -> Vec<OutputDecl> {
    let Some(node) = value else {
        return Vec::new();
    };
    let YamlData::Mapping(map) = &node.data else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(k, v)| {
            let name = as_str(k).filter(|s| !s.is_empty())?.to_string();
            Some(OutputDecl {
                name,
                value: get_field(v, "value")
                    .and_then(as_str)
                    .map(|s| s.to_string()),
            })
        })
        .collect()
}

pub(super) fn parse_secret_decls(value: Option<&MarkedYaml<'_>>) -> Vec<SecretDecl> {
    let Some(node) = value else {
        return Vec::new();
    };
    let YamlData::Mapping(map) = &node.data else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(k, v)| {
            let name = as_str(k).filter(|s| !s.is_empty())?.to_string();
            Some(SecretDecl {
                name,
                required: get_field(v, "required").and_then(as_bool).unwrap_or(false),
            })
        })
        .collect()
}
