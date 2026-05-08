use crate::ir::*;
use anyhow::{anyhow, Result};
use saphyr::{MarkedYaml, Scalar, YamlData};
use std::collections::BTreeMap;
use std::path::Path;

use super::helpers::{
    as_bool, as_str, get_field, parse_concurrency, parse_defaults, parse_permissions,
    parse_string_map, stringify_value,
};
use super::uses::{parse_uses, parse_workflow_ref};

pub(super) fn parse_jobs(
    value: Option<&MarkedYaml<'_>>,
    workflow_id: &WorkflowId,
    diags: &mut Vec<ParseDiagnostic>,
    file: &Path,
) -> Result<Vec<Job>> {
    let Some(node) = value else {
        return Ok(Vec::new());
    };
    let YamlData::Mapping(map) = &node.data else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (k, v) in map.iter() {
        let id = as_str(k)
            .ok_or_else(|| anyhow!("job key must be a string"))?
            .to_string();
        let line = k.span.start.line();
        out.push(parse_job(&id, v, workflow_id, line, diags, file)?);
    }
    Ok(out)
}

fn parse_job(
    id: &str,
    value: &MarkedYaml<'_>,
    workflow_id: &WorkflowId,
    source_line: usize,
    diags: &mut Vec<ParseDiagnostic>,
    file: &Path,
) -> Result<Job> {
    let path = file;
    let needs = match get_field(value, "needs").map(|v| &v.data) {
        Some(YamlData::Value(Scalar::String(s))) => vec![s.to_string()],
        Some(YamlData::Sequence(seq)) => seq
            .iter()
            .filter_map(|x| as_str(x).map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    };

    let permissions = parse_permissions(get_field(value, "permissions"), diags, file);

    let calls_workflow = if let Some(uses_val) = get_field(value, "uses") {
        if let Some(uses_str) = as_str(uses_val) {
            let workflow_ref = parse_workflow_ref(uses_str)?;
            let with = parse_string_map(get_field(value, "with"));
            let secrets = parse_secrets_pass(get_field(value, "secrets"));
            Some(CallsWorkflow {
                workflow_ref,
                with,
                secrets,
            })
        } else {
            None
        }
    } else {
        None
    };

    let runs_on = parse_runs_on(get_field(value, "runs-on"));

    // Reusable-workflow caller jobs must omit `runs-on` per spec; all other
    // jobs require it. Emit a non-fatal diagnostic rather than a hard error so
    // the rest of the IR is still usable.
    if calls_workflow.is_none() && runs_on.is_none() {
        diags.push(ParseDiagnostic {
            file: path.to_path_buf(),
            line: source_line,
            message: format!("job `{id}` is missing `runs-on`"),
        });
    }

    let steps = parse_steps(get_field(value, "steps"), path, diags);
    let outputs = parse_string_map(get_field(value, "outputs"));
    let environment = parse_environment(get_field(value, "environment"));
    let if_expr = get_field(value, "if")
        .and_then(as_str)
        .map(|s| s.to_string());
    let strategy = parse_strategy(get_field(value, "strategy"));
    let defaults = parse_defaults(get_field(value, "defaults"));
    let env = parse_string_map(get_field(value, "env"));
    let concurrency = parse_concurrency(get_field(value, "concurrency"));
    let container = parse_container(get_field(value, "container"));
    let services = parse_services(get_field(value, "services"));

    Ok(Job {
        id: JobId(id.to_string()),
        workflow: workflow_id.clone(),
        needs,
        permissions,
        steps,
        calls_workflow,
        runs_on,
        outputs,
        source: SourcePos {
            file: path.to_path_buf(),
            line: Some(source_line),
        },
        environment,
        if_expr,
        strategy,
        defaults,
        env,
        concurrency,
        container,
        services,
        annotations: Vec::new(),
    })
}

/// Parse `jobs.<job_id>.runs-on` into a [`RunsOn`] value.
///
/// Three forms are supported (Workflow syntax —
/// <https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax>):
///
/// - Scalar: `runs-on: ubuntu-latest`
/// - Sequence: `runs-on: [self-hosted, linux, x64]`
/// - Mapping: `runs-on: { group: my-runners, labels: [linux] }`
fn parse_runs_on(value: Option<&MarkedYaml<'_>>) -> Option<RunsOn> {
    let node = value?;
    match &node.data {
        // Scalar: `runs-on: ubuntu-latest`
        YamlData::Value(Scalar::String(s)) => Some(RunsOn {
            labels: vec![s.to_string()],
            group: None,
        }),
        // Sequence: `runs-on: [self-hosted, linux, x64]`
        YamlData::Sequence(seq) => {
            let labels = seq
                .iter()
                .filter_map(|x| as_str(x).map(|s| s.to_string()))
                .collect();
            Some(RunsOn {
                labels,
                group: None,
            })
        }
        // Mapping: `runs-on: { group: my-runners, labels: [linux] }`
        YamlData::Mapping(_) => {
            let group = get_field(node, "group")
                .and_then(as_str)
                .map(|s| s.to_string());
            let labels = get_field(node, "labels")
                .map(|n| match &n.data {
                    YamlData::Value(Scalar::String(s)) => vec![s.to_string()],
                    YamlData::Sequence(seq) => seq
                        .iter()
                        .filter_map(|x| as_str(x).map(|s| s.to_string()))
                        .collect(),
                    _ => Vec::new(),
                })
                .unwrap_or_default();
            Some(RunsOn { labels, group })
        }
        _ => None,
    }
}

/// Parse `jobs.<job_id>.strategy` into a [`Strategy`].
///
/// Spec reference: Workflow syntax —
/// https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax
/// section `jobs.<job_id>.strategy`
fn parse_strategy(value: Option<&MarkedYaml<'_>>) -> Option<Strategy> {
    let node = value?;
    let YamlData::Mapping(_) = &node.data else {
        return None;
    };

    let fail_fast = get_field(node, "fail-fast").and_then(as_bool);
    let max_parallel = get_field(node, "max-parallel").and_then(|v| {
        if let YamlData::Value(saphyr::Scalar::Integer(i)) = &v.data {
            u32::try_from(*i).ok()
        } else {
            None
        }
    });
    let matrix = parse_matrix(get_field(node, "matrix"));

    Some(Strategy {
        matrix,
        fail_fast,
        max_parallel,
    })
}

/// Parse `jobs.<job_id>.strategy.matrix` into a [`Matrix`].
///
/// Each key in the mapping maps to a sequence of [`MatrixValue`]s. The special
/// `include` and `exclude` keys are stored verbatim. Non-sequence values and
/// unrecognized structures are silently skipped.
fn parse_matrix(value: Option<&MarkedYaml<'_>>) -> Option<Matrix> {
    let node = value?;
    let YamlData::Mapping(map) = &node.data else {
        return None;
    };

    let mut dimensions = BTreeMap::new();
    for (k, v) in map.iter() {
        let key = match as_str(k) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let YamlData::Sequence(seq) = &v.data else {
            continue;
        };
        let values: Vec<MatrixValue> = seq.iter().filter_map(parse_matrix_value).collect();
        dimensions.insert(key, values);
    }

    Some(Matrix { dimensions })
}

/// Convert a single YAML node into a [`MatrixValue`].
///
/// Handles: strings, integers, booleans, and mapping objects (recursing one
/// level). Null and sequence nodes are ignored (return `None`).
fn parse_matrix_value(node: &MarkedYaml<'_>) -> Option<MatrixValue> {
    match &node.data {
        YamlData::Value(saphyr::Scalar::String(s)) => Some(MatrixValue::String(s.to_string())),
        YamlData::Value(saphyr::Scalar::Integer(i)) => Some(MatrixValue::Int(*i)),
        YamlData::Value(saphyr::Scalar::Boolean(b)) => Some(MatrixValue::Bool(*b)),
        YamlData::Mapping(map) => {
            let mut obj = BTreeMap::new();
            for (k, v) in map.iter() {
                let key = as_str(k)?.to_string();
                if let Some(val) = parse_matrix_value(v) {
                    obj.insert(key, val);
                }
            }
            Some(MatrixValue::Object(obj))
        }
        _ => None,
    }
}

fn parse_environment(value: Option<&MarkedYaml<'_>>) -> Option<JobEnvironment> {
    let node = value?;
    match &node.data {
        YamlData::Value(Scalar::String(s)) => Some(JobEnvironment {
            name: s.to_string(),
            url: None,
        }),
        YamlData::Mapping(map) => {
            let name = map.iter().find_map(|(k, v)| {
                if as_str(k)? == "name" {
                    as_str(v).map(|s| s.to_string())
                } else {
                    None
                }
            })?;
            let url = map.iter().find_map(|(k, v)| {
                if as_str(k)? == "url" {
                    as_str(v).map(|s| s.to_string())
                } else {
                    None
                }
            });
            Some(JobEnvironment { name, url })
        }
        _ => None,
    }
}

/// Parse `container:` at either scalar (`container: alpine:3.20`) or mapping form.
///
/// Ref: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idcontainer
fn parse_container(value: Option<&MarkedYaml<'_>>) -> Option<JobContainer> {
    let node = value?;
    match &node.data {
        // Scalar: `container: alpine:3.20`
        YamlData::Value(Scalar::String(s)) => Some(JobContainer {
            image: s.to_string(),
            credentials: None,
            env: BTreeMap::new(),
            ports: Vec::new(),
            volumes: Vec::new(),
            options: None,
        }),
        // Mapping: `container:\n  image: alpine:3.20\n  ...`
        YamlData::Mapping(_) => {
            let image = get_field(node, "image").and_then(as_str)?.to_string();
            let credentials = parse_container_credentials(get_field(node, "credentials"));
            let env = parse_string_map(get_field(node, "env"));
            let ports = parse_str_list(get_field(node, "ports"));
            let volumes = parse_str_list(get_field(node, "volumes"));
            let options = get_field(node, "options")
                .and_then(as_str)
                .map(|s| s.to_string());
            Some(JobContainer {
                image,
                credentials,
                env,
                ports,
                volumes,
                options,
            })
        }
        _ => None,
    }
}

/// Parse `credentials:` sub-mapping inside a container definition.
fn parse_container_credentials(value: Option<&MarkedYaml<'_>>) -> Option<JobContainerCredentials> {
    let node = value?;
    let username = get_field(node, "username").and_then(as_str)?.to_string();
    let password = get_field(node, "password").and_then(as_str)?.to_string();
    Some(JobContainerCredentials { username, password })
}

/// Parse `services:` mapping — each value is a container mapping (no scalar form per spec).
///
/// Ref: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idservices
fn parse_services(value: Option<&MarkedYaml<'_>>) -> BTreeMap<String, JobContainer> {
    let Some(node) = value else {
        return BTreeMap::new();
    };
    let YamlData::Mapping(map) = &node.data else {
        return BTreeMap::new();
    };
    map.iter()
        .filter_map(|(k, v)| {
            let name = as_str(k)?.to_string();
            let container = parse_container(Some(v))?;
            Some((name, container))
        })
        .collect()
}

/// Parse a field that may be a single string or a sequence of strings into a `Vec<String>`.
fn parse_str_list(value: Option<&MarkedYaml<'_>>) -> Vec<String> {
    let Some(node) = value else {
        return Vec::new();
    };
    match &node.data {
        YamlData::Value(Scalar::String(s)) => vec![s.to_string()],
        YamlData::Sequence(items) => items
            .iter()
            .filter_map(|x| as_str(x).map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn parse_steps(
    value: Option<&MarkedYaml<'_>>,
    file: &Path,
    diags: &mut Vec<ParseDiagnostic>,
) -> Vec<Step> {
    let Some(node) = value else {
        return Vec::new();
    };
    let YamlData::Sequence(seq) = &node.data else {
        return Vec::new();
    };
    seq.iter()
        .enumerate()
        .map(|(index, step)| Step {
            index,
            id: get_field(step, "id")
                .and_then(as_str)
                .map(|s| StepId(s.to_string())),
            name: get_field(step, "name")
                .and_then(as_str)
                .map(|s| s.to_string()),
            uses: get_field(step, "uses").and_then(|uses_node| {
                let s = as_str(uses_node)?;
                match parse_uses(s) {
                    Ok(uses_ref) => Some(uses_ref),
                    Err(err) => {
                        diags.push(ParseDiagnostic {
                            file: file.to_path_buf(),
                            line: uses_node.span.start.line(),
                            message: format!("invalid step `uses`: {err}"),
                        });
                        None
                    }
                }
            }),
            run: get_field(step, "run")
                .and_then(as_str)
                .map(|s| s.to_string()),
            if_expr: get_field(step, "if")
                .and_then(as_str)
                .map(|s| s.to_string()),
            with: parse_string_map(get_field(step, "with")),
            env: parse_string_map(get_field(step, "env")),
            source: SourcePos {
                file: file.to_path_buf(),
                line: Some(step.span.start.line()),
            },
            shell: get_field(step, "shell")
                .and_then(as_str)
                .map(|s| s.to_string()),
            working_directory: get_field(step, "working-directory")
                .and_then(as_str)
                .map(|s| s.to_string()),
            timeout_minutes: get_field(step, "timeout-minutes").and_then(|n| {
                if let YamlData::Value(Scalar::Integer(i)) = &n.data {
                    u32::try_from(*i).ok()
                } else {
                    None
                }
            }),
            continue_on_error: get_field(step, "continue-on-error")
                .map(stringify_value)
                .filter(|s| !s.is_empty() && s != "null"),
            annotations: Vec::new(),
        })
        .collect()
}

fn parse_secrets_pass(value: Option<&MarkedYaml<'_>>) -> SecretsPass {
    let Some(node) = value else {
        return SecretsPass::None;
    };
    match &node.data {
        YamlData::Value(Scalar::String(s)) if s == "inherit" => SecretsPass::Inherit,
        YamlData::Mapping(map) => {
            let mut explicit = BTreeMap::new();
            for (k, v) in map.iter() {
                if let Some(key) = as_str(k) {
                    explicit.insert(key.to_string(), stringify_value(v));
                }
            }
            SecretsPass::Explicit(explicit)
        }
        _ => SecretsPass::None,
    }
}
