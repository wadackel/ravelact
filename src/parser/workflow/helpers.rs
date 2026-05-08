use crate::ir::*;
use anyhow::{Context, Result};
use saphyr::{MarkedYaml, Scalar, YamlData};
use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn rel_id(path: &Path, root: &Path) -> Result<String> {
    let rel = path.strip_prefix(root).with_context(|| {
        format!(
            "workflow {} not under root {}",
            path.display(),
            root.display()
        )
    })?;
    Ok(path_to_forward(rel))
}

pub(crate) fn path_to_forward(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn get_field<'a>(node: &'a MarkedYaml<'_>, key: &str) -> Option<&'a MarkedYaml<'a>> {
    let YamlData::Mapping(map) = &node.data else {
        return None;
    };
    for (k, v) in map.iter() {
        if let YamlData::Value(Scalar::String(s)) = &k.data {
            if s == key {
                return Some(v);
            }
        }
    }
    None
}

pub(crate) fn as_str<'a>(node: &'a MarkedYaml<'a>) -> Option<&'a str> {
    if let YamlData::Value(Scalar::String(s)) = &node.data {
        Some(s.as_ref())
    } else {
        None
    }
}

pub(crate) fn as_bool(node: &MarkedYaml<'_>) -> Option<bool> {
    if let YamlData::Value(Scalar::Boolean(b)) = &node.data {
        Some(*b)
    } else {
        None
    }
}

pub(super) fn string_field(node: &MarkedYaml<'_>, key: &str) -> Option<String> {
    get_field(node, key).and_then(as_str).map(|s| s.to_string())
}

/// Parse a field that may be a single string or a sequence of strings into a `Vec<String>`.
pub(super) fn str_list_from(node: &MarkedYaml<'_>) -> Vec<String> {
    match &node.data {
        YamlData::Value(Scalar::String(s)) => vec![s.to_string()],
        YamlData::Sequence(items) => items
            .iter()
            .filter_map(|x| as_str(x).map(|s| s.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn parse_string_map(value: Option<&MarkedYaml<'_>>) -> BTreeMap<String, String> {
    let Some(node) = value else {
        return BTreeMap::new();
    };
    let YamlData::Mapping(map) = &node.data else {
        return BTreeMap::new();
    };
    map.iter()
        .filter_map(|(k, v)| {
            let key = as_str(k)?.to_string();
            Some((key, stringify_value(v)))
        })
        .collect()
}

pub(crate) fn stringify_value(v: &MarkedYaml<'_>) -> String {
    match &v.data {
        YamlData::Value(Scalar::String(s)) => s.to_string(),
        YamlData::Value(Scalar::Boolean(b)) => b.to_string(),
        YamlData::Value(Scalar::Integer(i)) => i.to_string(),
        YamlData::Value(Scalar::FloatingPoint(f)) => f.to_string(),
        YamlData::Value(Scalar::Null) => "null".to_string(),
        _ => String::new(),
    }
}

/// Parse a `defaults:` block (workflow- or job-level) into a [`Defaults`] value.
/// Returns `None` when the key is absent or does not contain a mapping.
/// Ref: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#defaults
pub(super) fn parse_defaults(value: Option<&MarkedYaml<'_>>) -> Option<Defaults> {
    let node = value?;
    let run_node = get_field(node, "run")?;
    let shell = string_field(run_node, "shell");
    let working_directory = string_field(run_node, "working-directory");
    if shell.is_none() && working_directory.is_none() {
        return Some(Defaults {
            run: Some(RunDefaults::default()),
        });
    }
    Some(Defaults {
        run: Some(RunDefaults {
            shell,
            working_directory,
        }),
    })
}

pub(super) fn parse_permissions(
    value: Option<&MarkedYaml<'_>>,
    diags: &mut Vec<ParseDiagnostic>,
    file: &Path,
) -> Option<Permissions> {
    let node = value?;
    match &node.data {
        YamlData::Value(Scalar::String(s)) => {
            let kind = match s.as_ref() {
                "read-all" => CoarseKind::ReadAll,
                "write-all" => CoarseKind::WriteAll,
                other => {
                    diags.push(ParseDiagnostic {
                        file: file.to_path_buf(),
                        line: node.span.start.line(),
                        message: format!(
                            "unknown coarse permissions value `{other}`; expected `read-all` or `write-all`"
                        ),
                    });
                    CoarseKind::Unknown(other.to_string())
                }
            };
            Some(Permissions::Coarse(kind))
        }
        YamlData::Mapping(map) => {
            let mut scopes = BTreeMap::new();
            for (k, v) in map.iter() {
                let Some(key_str) = as_str(k) else { continue };
                let Some(val_str) = as_str(v) else { continue };

                let scope_key = parse_scope_key(key_str).unwrap_or_else(|| {
                    diags.push(ParseDiagnostic {
                        file: file.to_path_buf(),
                        line: k.span.start.line(),
                        message: format!("unknown permissions scope key `{key_str}`"),
                    });
                    ScopeKey::Unknown(key_str.to_string())
                });

                let scope_access = match val_str {
                    "read" => ScopeAccess::Read,
                    "write" => ScopeAccess::Write,
                    "none" => ScopeAccess::None,
                    other => {
                        diags.push(ParseDiagnostic {
                            file: file.to_path_buf(),
                            line: v.span.start.line(),
                            message: format!(
                                "unknown permissions access value `{other}` for scope `{key_str}`; expected `read`, `write`, or `none`"
                            ),
                        });
                        ScopeAccess::Unknown(other.to_string())
                    }
                };

                scopes.insert(scope_key, scope_access);
            }
            Some(Permissions::Scopes(scopes))
        }
        _ => None,
    }
}

/// Map a YAML permissions scope key string to a typed [`ScopeKey`].
/// Returns `None` for unrecognized keys (caller emits a diagnostic and uses
/// [`ScopeKey::Unknown`]).
fn parse_scope_key(s: &str) -> Option<ScopeKey> {
    match s {
        "actions" => Some(ScopeKey::Actions),
        "artifact-metadata" => Some(ScopeKey::ArtifactMetadata),
        "attestations" => Some(ScopeKey::Attestations),
        "checks" => Some(ScopeKey::Checks),
        "contents" => Some(ScopeKey::Contents),
        "deployments" => Some(ScopeKey::Deployments),
        "discussions" => Some(ScopeKey::Discussions),
        "id-token" => Some(ScopeKey::IdToken),
        "issues" => Some(ScopeKey::Issues),
        "models" => Some(ScopeKey::Models),
        "packages" => Some(ScopeKey::Packages),
        "pages" => Some(ScopeKey::Pages),
        "pull-requests" => Some(ScopeKey::PullRequests),
        "repository-projects" => Some(ScopeKey::RepositoryProjects),
        "security-events" => Some(ScopeKey::SecurityEvents),
        "statuses" => Some(ScopeKey::Statuses),
        "vulnerability-alerts" => Some(ScopeKey::VulnerabilityAlerts),
        _ => None,
    }
}

/// Parse a `concurrency:` value into a [`Concurrency`] IR node.
///
/// Accepts both the scalar shorthand (`concurrency: my-group`) and the map form
/// (`concurrency: { group: …, cancel-in-progress: … }`), per the GitHub Actions
/// spec — https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency
pub(super) fn parse_concurrency(value: Option<&MarkedYaml<'_>>) -> Option<Concurrency> {
    let node = value?;
    match &node.data {
        // Scalar shorthand: `concurrency: my-group`
        YamlData::Value(Scalar::String(s)) => Some(Concurrency {
            group: s.to_string(),
            cancel_in_progress: None,
        }),
        // Map form: `concurrency: { group: …, cancel-in-progress: … }`
        YamlData::Mapping(_) => {
            let group = get_field(node, "group").and_then(as_str)?.to_string();
            let cancel_in_progress = get_field(node, "cancel-in-progress").and_then(as_bool);
            Some(Concurrency {
                group,
                cancel_in_progress,
            })
        }
        _ => None,
    }
}
