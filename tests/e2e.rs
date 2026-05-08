//! End-to-end tests against hand-crafted synthetic workflow estates under
//! `tests/fixtures/synthetic/`. Each fixture targets a specific structural
//! feature ravelact must analyze (matrix expansion, reusable workflows,
//! Docker actions, non-standard composite paths, dedup-able clusters, etc.).

use assert_cmd::Command;
use serde_json::Value;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

mod common;
use common::test_state_dir;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic")
}

/// Sorted list of fixture directory names under `tests/fixtures/synthetic/`.
/// Top-level files (e.g. `README.md`) are skipped.
fn fixtures() -> Vec<String> {
    let root = fixtures_dir();
    if !root.exists() {
        return Vec::new();
    }
    let mut out: Vec<String> = std::fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read {}: {e}", root.display()))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    out.sort();
    out
}

fn copy_to_tempdir(name: &str) -> TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = fixtures_dir().join(name);
    copy_tree(&src, tmp.path());
    tmp
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

fn run(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    cmd.arg("--root").arg(root).args(args);
    cmd.assert()
}

fn run_capture(root: &Path, args: &[&str]) -> String {
    let assert = run(root, args).success();
    String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout")
}

/// Capture stdout regardless of exit code (`check inputs` exits 1 when findings exist).
fn run_capture_any(root: &Path, args: &[&str]) -> String {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    cmd.arg("--root").arg(root).args(args);
    let output = cmd.output().expect("spawn ravelact");
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

/// Strip machine-dependent fields from an IR JSON dump and emit a stable,
/// sorted projection suitable for snapshot diffing.
fn portable_projection(ir: &Value) -> Value {
    let mut workflows: Vec<Value> = ir
        .get("workflows")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(workflow_projection).collect())
        .unwrap_or_default();
    workflows.sort_by(|a, b| {
        a.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });

    let mut actions: Vec<Value> = ir
        .get("actions")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(composite_projection).collect())
        .unwrap_or_default();
    actions.sort_by(|a, b| {
        a.get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .cmp(b.get("id").and_then(|v| v.as_str()).unwrap_or(""))
    });

    let mut externals: Vec<Value> = ir
        .get("external_actions")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(external_projection).collect())
        .unwrap_or_default();
    externals.sort_by(|a, b| {
        let key = |v: &Value| -> String {
            format!(
                "{}/{}/{}@{}",
                v.get("owner").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("repo").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("subpath").and_then(|x| x.as_str()).unwrap_or(""),
                v.get("gitref").and_then(|x| x.as_str()).unwrap_or("")
            )
        };
        key(a).cmp(&key(b))
    });

    serde_json::json!({
        "workflows": workflows,
        "actions": actions,
        "externals": externals,
        "schema_version": ir.get("schema_version").cloned().unwrap_or(Value::Null),
    })
}

fn workflow_projection(wf: &Value) -> Value {
    let id = wf.get("id").cloned().unwrap_or(Value::Null);
    let name = wf.get("name").cloned().unwrap_or(Value::Null);
    let mut triggers: Vec<String> = wf
        .get("triggers")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(trigger_event_name).collect())
        .unwrap_or_default();
    triggers.sort();
    triggers.dedup();

    let jobs: Vec<Value> = wf
        .get("jobs")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(job_projection).collect())
        .unwrap_or_default();

    serde_json::json!({
        "id": id,
        "name": name,
        "triggers": triggers,
        "jobs": jobs,
    })
}

fn trigger_event_name(t: &Value) -> Option<String> {
    if let Some(s) = t.as_str() {
        return Some(s.to_string());
    }
    let obj = t.as_object()?;
    let event = obj.get("event")?.as_object()?;
    let kind = event.get("kind")?.as_str()?;
    if kind == "other" {
        let name = event
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("other");
        Some(name.to_string())
    } else {
        Some(kind.to_string())
    }
}

fn job_projection(job: &Value) -> Value {
    let id = job.get("id").cloned().unwrap_or(Value::Null);
    let calls_obj = job.get("calls_workflow").and_then(|v| v.as_object());
    let calls = calls_obj
        .and_then(|obj| obj.get("workflow_ref"))
        .map(workflow_ref_label)
        .unwrap_or(Value::Null);
    // SecretsPass serializes as `"None"` / `"Inherit"` (string) for unit variants
    // or `{"Explicit": {...}}` for the map variant. We project only the inherit
    // signal — `null` covers None / Explicit / missing.
    let calls_secrets = calls_obj
        .and_then(|obj| obj.get("secrets"))
        .and_then(|v| v.as_str())
        .filter(|s| *s == "Inherit")
        .map(|_| Value::String("inherit".into()))
        .unwrap_or(Value::Null);
    let mut step_kinds: Vec<String> = job
        .get("steps")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(step_use_kind).collect())
        .unwrap_or_default();
    step_kinds.sort();
    serde_json::json!({
        "id": id,
        "calls_workflow": calls,
        "calls_secrets": calls_secrets,
        "step_uses_kinds": step_kinds,
    })
}

fn workflow_ref_label(r: &Value) -> Value {
    if let Some(obj) = r.as_object() {
        if let Some(local) = obj.get("Local") {
            return Value::String(format!("local:{}", local.as_str().unwrap_or("?")));
        }
        if let Some(ext) = obj.get("External").and_then(|v| v.as_object()) {
            let owner = ext.get("owner").and_then(|v| v.as_str()).unwrap_or("?");
            let repo = ext.get("repo").and_then(|v| v.as_str()).unwrap_or("?");
            let path = ext.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let gitref = ext.get("gitref").and_then(|v| v.as_str()).unwrap_or("?");
            return Value::String(format!("external:{owner}/{repo}/{path}@{gitref}"));
        }
    }
    Value::Null
}

fn step_use_kind(step: &Value) -> Option<String> {
    let uses = step.get("uses")?;
    if uses.is_null() {
        return None;
    }
    if let Some(obj) = uses.as_object() {
        for k in ["LocalWorkflow", "LocalAction", "External", "Docker"] {
            if obj.contains_key(k) {
                return Some(k.to_lowercase());
            }
        }
    }
    None
}

fn composite_projection(c: &Value) -> Value {
    let id = c.get("id").cloned().unwrap_or(Value::Null);
    let kind = match c.get("kind") {
        // Unit variants serialize as bare strings (`"Composite"`, `"Docker"`).
        // Lowercase them so the projection matches the lowercased prefix used
        // for `JavaScript { node_version }` (`"javascript:nodeXX"`).
        Some(Value::String(s)) => s.to_lowercase(),
        Some(Value::Object(obj)) => {
            if let Some(js) = obj.get("JavaScript").and_then(|v| v.as_object()) {
                let v = js
                    .get("node_version")
                    .and_then(|x| x.as_str())
                    .unwrap_or("?");
                format!("javascript:{v}")
            } else if obj.contains_key("Composite") {
                "composite".into()
            } else if obj.contains_key("Docker") {
                "docker".into()
            } else {
                "unknown".into()
            }
        }
        _ => "unknown".into(),
    };
    serde_json::json!({ "id": id, "kind": kind })
}

fn external_projection(e: &Value) -> Value {
    serde_json::json!({
        "owner": e.get("owner").cloned().unwrap_or(Value::Null),
        "repo": e.get("repo").cloned().unwrap_or(Value::Null),
        "subpath": e.get("subpath").cloned().unwrap_or(Value::Null),
        "gitref": e.get("gitref").cloned().unwrap_or(Value::Null),
    })
}

#[test]
fn build_succeeds_for_all_fixtures() {
    let names = fixtures();
    if names.is_empty() {
        eprintln!("no fixtures present yet — skipping");
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        run(tmp.path(), &["build"]).success();
    }
}

#[test]
fn dump_summary_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let raw = run_capture(tmp.path(), &["dump"]);
        let ir: Value = serde_json::from_str(&raw).expect("valid JSON");
        let projection = portable_projection(&ir);
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_json_snapshot!("dump", projection);
        });
    }
}

#[test]
fn orphans_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = run_capture(tmp.path(), &["orphans"]);
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("orphans", out);
        });
    }
}

#[test]
fn orphans_markdown_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = run_capture(tmp.path(), &["orphans", "--format", "markdown"]);
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("orphans_markdown", out);
        });
    }
}

#[test]
fn extract_markdown_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = run_capture(tmp.path(), &["extract", "--format", "markdown"]);
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("extract_markdown", out);
        });
    }
}

#[test]
fn dedup_markdown_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = run_capture(tmp.path(), &["dedup", "--format", "markdown"]);
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("dedup_markdown", out);
        });
    }
}

#[test]
fn trace_push_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = run_capture(tmp.path(), &["trace", "push"]);
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("trace_push", out);
        });
    }
}

#[test]
fn graph_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = run_capture(tmp.path(), &["graph"]);
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("graph", out);
        });
    }
}

#[test]
fn check_permissions_snapshot_all() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let canonical = tmp.path().canonicalize().expect("canonicalize tmp");
        let prefix = canonical.to_string_lossy().into_owned();
        let raw = run_capture_any(tmp.path(), &["permissions"]);
        let out = raw.replace(&prefix, "<TMPDIR>");
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("check_permissions", out);
        });
    }
}

#[test]
fn check_secrets_snapshot_all() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let canonical = tmp.path().canonicalize().expect("canonicalize tmp");
        let prefix = canonical.to_string_lossy().into_owned();
        let raw = run_capture_any(tmp.path(), &["secrets"]);
        let out = raw.replace(&prefix, "<TMPDIR>");
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("check_secrets", out);
        });
    }
}

#[test]
fn wiring_snapshot_all() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let canonical = tmp.path().canonicalize().expect("canonicalize tmp");
        let prefix = canonical.to_string_lossy().into_owned();
        let raw = run_capture_any(tmp.path(), &["wiring"]);
        let out = raw.replace(&prefix, "<TMPDIR>");
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("wiring", out);
        });
    }
}

/// Selector for the `callers` universal loop.
///
/// Rule: build the IR, list every workflow whose triggers include
/// `workflow_call` (i.e. it is a reusable workflow callable from another
/// workflow), sort by full repo-relative path lexicographically, and pick
/// the first. If no workflow is reusable, return None and the loop emits a
/// `(no eligible target)` snapshot so absence is itself snapshotted rather
/// than silently skipped.
fn select_callers_target(root: &Path) -> Option<String> {
    let raw = run_capture(root, &["dump"]);
    let ir: Value = serde_json::from_str(&raw).ok()?;
    let workflows = ir.get("workflows")?.as_array()?;
    let mut reusable: Vec<String> = workflows
        .iter()
        .filter(|wf| {
            wf.get("triggers")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .any(|t| trigger_event_name(t).as_deref() == Some("workflow_call"))
                })
                .unwrap_or(false)
        })
        .filter_map(|wf| wf.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    reusable.sort();
    reusable.into_iter().next()
}

/// Selector for the `impact` universal loop.
///
/// Rule: build the IR, list every workflow's repo-relative path, sort
/// lexicographically, and pick the first. Every fixture has at least one
/// workflow, so `None` is unexpected; we still emit `(no eligible target)`
/// for safety so an empty fixture would yield a deterministic snapshot
/// rather than a panic.
fn select_impact_target(root: &Path) -> Option<String> {
    let raw = run_capture(root, &["dump"]);
    let ir: Value = serde_json::from_str(&raw).ok()?;
    let workflows = ir.get("workflows")?.as_array()?;
    let mut paths: Vec<String> = workflows
        .iter()
        .filter_map(|wf| wf.get("id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    paths.sort();
    paths.into_iter().next()
}

#[test]
fn callers_snapshot_all() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = match select_callers_target(tmp.path()) {
            Some(target) => run_capture(tmp.path(), &["callers", &target]),
            None => "(no eligible target)\n".to_string(),
        };
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("callers", out);
        });
    }
}

#[test]
fn impact_snapshot_all() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = match select_impact_target(tmp.path()) {
            Some(target) => run_capture(tmp.path(), &["impact", &target]),
            None => "(no eligible target)\n".to_string(),
        };
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("impact", out);
        });
    }
}

#[test]
fn impact_reusable_lists_callers() {
    let name = "cross-repo-call";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture missing — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let stdout = run_capture(
        tmp.path(),
        &[
            "impact",
            ".github/workflows/_reusable.yml",
            "--format",
            "json",
        ],
    );
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let workflows = v["workflows"].as_array().expect("workflows is array");
    let names: Vec<&str> = workflows.iter().filter_map(|x| x.as_str()).collect();

    assert!(
        names.contains(&".github/workflows/entry-a.yml"),
        "entry-a.yml must appear in impacted entry-points: {stdout}"
    );
    assert!(
        names.contains(&".github/workflows/entry-b.yml"),
        "entry-b.yml must appear in impacted entry-points: {stdout}"
    );
    assert!(
        !names.contains(&".github/workflows/_reusable.yml"),
        "_reusable.yml is workflow_call-only and must NOT appear in entry-points: {stdout}"
    );
    assert_eq!(
        names.len(),
        2,
        "expected exactly 2 entry-point callers, got {}: {stdout}",
        names.len()
    );
}

#[test]
fn graph_snapshot_cross_repo_call() {
    let name = "cross-repo-call";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture absent — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let out = run_capture(tmp.path(), &["graph"]);
    assert!(
        out.starts_with("%% generated by ravelact\ngraph LR\n"),
        "mermaid output must lead with header, got:\n{out}"
    );
    insta::with_settings!({snapshot_suffix => name.to_string()}, {
        insta::assert_snapshot!("graph", out);
    });
}

#[test]
fn graph_snapshot_workflow_run_chain() {
    // The workflow-run-chain fixture is the only place where the 3-deep
    // workflow_run name-match resolution is observable. trace push only sees
    // the entry-point (`trigger.yml`) because walk_workflow does not traverse
    // workflow_run triggers — graph is the only query that resolves them.
    let name = "workflow-run-chain";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture absent — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let out = run_capture(tmp.path(), &["graph"]);
    assert!(
        out.starts_with("%% generated by ravelact\ngraph LR\n"),
        "mermaid output must lead with header, got:\n{out}"
    );
    insta::with_settings!({snapshot_suffix => name.to_string()}, {
        insta::assert_snapshot!("graph", out);
    });
}

#[test]
fn graph_snapshot_composite_annotations() {
    // The composite-annotations fixture exercises annotations carried by a
    // composite action manifest and by a composite step. The graph must
    // include dotted annotated edges originating from the action node.
    let name = "composite-annotations";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture absent — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let out = run_capture(tmp.path(), &["graph"]);
    assert!(
        out.starts_with("%% generated by ravelact\ngraph LR\n"),
        "mermaid output must lead with header, got:\n{out}"
    );
    assert!(
        out.contains("act_0 -. triggers .->"),
        "composite-action manifest annotation must emit a dotted triggers edge: {out}"
    );
    assert!(
        out.contains("act_0 -. dispatches .->"),
        "composite-step annotation must emit a dotted dispatches edge: {out}"
    );
    insta::with_settings!({snapshot_suffix => name.to_string()}, {
        insta::assert_snapshot!("graph", out);
    });
}

#[test]
fn callers_snapshot_composite_annotations() {
    // Both the manifest-level annotation (`triggers manifest-target.yml`) and
    // the composite-step annotation (`dispatches step-target.yml`) must
    // surface as `annotated-composite` caller hits.
    let name = "composite-annotations";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture absent — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let manifest = run_capture(
        tmp.path(),
        &["callers", ".github/workflows/manifest-target.yml"],
    );
    let step = run_capture(
        tmp.path(),
        &["callers", ".github/workflows/step-target.yml"],
    );
    assert!(
        manifest.contains("annotated-composite") && manifest.contains("_action via triggers"),
        "manifest-level annotation must surface as annotated-composite/_action via triggers: {manifest}"
    );
    assert!(
        step.contains("annotated-composite") && step.contains("_composite:0 via dispatches"),
        "composite-step annotation must surface as annotated-composite/_composite:0 via dispatches: {step}"
    );
    insta::with_settings!({snapshot_suffix => format!("{name}_manifest")}, {
        insta::assert_snapshot!("callers", manifest);
    });
    insta::with_settings!({snapshot_suffix => format!("{name}_step")}, {
        insta::assert_snapshot!("callers", step);
    });
}

#[test]
fn impact_snapshot_composite_annotations() {
    // Changing the workflow that the composite action's annotation targets
    // must propagate up through the composite to every workflow using it.
    let name = "composite-annotations";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture absent — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let manifest = run_capture(
        tmp.path(),
        &["impact", ".github/workflows/manifest-target.yml"],
    );
    let step = run_capture(tmp.path(), &["impact", ".github/workflows/step-target.yml"]);
    assert!(
        manifest.contains(".github/workflows/entry.yml")
            && manifest.contains(".github/actions/notify"),
        "impact must propagate composite-manifest annotation to entry.yml + notify: {manifest}"
    );
    assert!(
        step.contains(".github/workflows/entry.yml") && step.contains(".github/actions/notify"),
        "impact must propagate composite-step annotation to entry.yml + notify: {step}"
    );
    insta::with_settings!({snapshot_suffix => format!("{name}_manifest")}, {
        insta::assert_snapshot!("impact", manifest);
    });
    insta::with_settings!({snapshot_suffix => format!("{name}_step")}, {
        insta::assert_snapshot!("impact", step);
    });
}

#[test]
fn wiring_snapshot_composite_annotations() {
    // All composite annotations in the fixture resolve to existing workflows,
    // so wiring must report no findings and exit 0.
    let name = "composite-annotations";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture absent — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let out = run_capture(tmp.path(), &["wiring"]);
    assert!(
        out.contains("wiring  no findings"),
        "fixture has no dangling composite annotations and no unannotated dispatches; wiring must report empty: {out}"
    );
    insta::with_settings!({snapshot_suffix => name.to_string()}, {
        insta::assert_snapshot!("wiring", out);
    });
}

#[test]
fn suggest_extract_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let out = run_capture(tmp.path(), &["extract"]);
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("suggest_extract", out);
        });
    }
}

#[test]
fn suggest_extract_smoke() {
    let name = "large-estate";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture missing — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let raw = run_capture(tmp.path(), &["extract", "--format", "json"]);
    let v: Value = serde_json::from_str(&raw).expect("valid JSON");
    assert!(v.is_array(), "top-level must be a JSON array, got: {raw}");
}

/// Regression coverage for #113: emit valid `uses:` lines for local refs.
///
/// 1. Local-action sketches must use the `./<path>` form so the generated
///    composite is valid GitHub Actions syntax.
/// 2. A duplicate window that crosses a `LocalWorkflow` step must be excluded
///    (reusable workflows cannot live inside a composite).
#[test]
fn suggest_extract_local_refs_emit_valid_uses() {
    let name = "extract-local-refs";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture missing — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let raw = run_capture(tmp.path(), &["extract", "--format", "json"]);
    let v: Value = serde_json::from_str(&raw).expect("valid JSON");
    let arr = v.as_array().expect("top-level must be a JSON array");

    // Must produce at least one candidate covering the 3-step bootstrap that
    // ends just before the reusable-workflow wall.
    assert!(
        !arr.is_empty(),
        "expected at least one candidate from extract-local-refs, got empty array"
    );

    let local_action_hits = arr
        .iter()
        .filter(|c| {
            c["sketch"]
                .as_str()
                .map(|s| s.contains("uses: ./.github/actions/setup"))
                .unwrap_or(false)
        })
        .count();
    assert!(
        local_action_hits >= 1,
        "expected at least one candidate sketch with `uses: ./.github/actions/setup`; got: {raw}"
    );

    for c in arr {
        let sketch = c["sketch"].as_str().unwrap_or("");
        assert!(
            !sketch.contains("uses: .github/actions/setup"),
            "candidate sketch must not emit a bare local-action path:\n{sketch}"
        );
        assert!(
            !sketch.contains("reusable.yml"),
            "candidate sketch must not include a reusable-workflow ref:\n{sketch}"
        );
    }
}

#[test]
fn wiring_surfaces_dangling_local_uses_fixture() {
    // Issue #111: a fixture referencing `./.github/actions/typo` with no
    // action manifest present must produce a `DanglingLocalUses` wiring
    // finding (acceptance bullet 1).
    let name = "dangling-local-uses";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture absent — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);

    // `wiring` exits 1 when findings are reported (Check-group contract).
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    cmd.arg("--root")
        .arg(tmp.path())
        .args(["wiring", "--format", "json"]);
    let output = cmd.output().expect("spawn ravelact");
    assert_eq!(
        output.status.code(),
        Some(1),
        "wiring must exit 1 when findings are reported; stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).expect("utf8");
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = v.as_array().expect("top-level array");
    let dangling: Vec<&Value> = arr
        .iter()
        .filter(|f| f.get("kind").and_then(Value::as_str) == Some("DanglingLocalUses"))
        .collect();
    assert_eq!(
        dangling.len(),
        1,
        "expected exactly one DanglingLocalUses finding, got: {stdout}"
    );
    let finding = dangling[0];
    assert_eq!(
        finding.get("local_kind").and_then(Value::as_str),
        Some("Action"),
        "expected local_kind=Action: {finding}"
    );
    assert_eq!(
        finding.get("raw_target").and_then(Value::as_str),
        Some(".github/actions/typo"),
        "expected raw_target=.github/actions/typo: {finding}"
    );
}

#[test]
fn graph_handles_dangling_local_uses_fixture() {
    // Issue #111 acceptance bullet 2: `graph` against the dangling-local-uses
    // fixture exits successfully with deterministic output (no panic on the
    // unresolved local-action id).
    let name = "dangling-local-uses";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture absent — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let out = run_capture(tmp.path(), &["graph"]);
    assert!(
        out.starts_with("%% generated by ravelact\ngraph LR\n"),
        "mermaid output must lead with header, got:\n{out}"
    );
    insta::with_settings!({snapshot_suffix => name.to_string()}, {
        insta::assert_snapshot!("graph", out);
    });
}

#[test]
fn check_permissions_snapshot() {
    let scope = ["cross-repo-call", "large-estate"];
    for name in scope {
        if !fixtures_dir().join(name).is_dir() {
            eprintln!("{name} fixture missing — skipping");
            continue;
        }
        let tmp = copy_to_tempdir(name);
        let canonical = tmp.path().canonicalize().expect("canonicalize tmp");
        let prefix = canonical.to_string_lossy().into_owned();
        let raw = run_capture_any(tmp.path(), &["permissions"]);
        let out = raw.replace(&prefix, "<TMPDIR>");
        insta::with_settings!({snapshot_suffix => name.to_string()}, {
            insta::assert_snapshot!("check_permissions", out);
        });
    }
}

#[test]
fn check_secrets_snapshot() {
    let scope = ["cross-repo-call", "large-estate"];
    for name in scope {
        if !fixtures_dir().join(name).is_dir() {
            eprintln!("{name} fixture missing — skipping");
            continue;
        }
        let tmp = copy_to_tempdir(name);
        let canonical = tmp.path().canonicalize().expect("canonicalize tmp");
        let prefix = canonical.to_string_lossy().into_owned();
        let raw = run_capture_any(tmp.path(), &["secrets"]);
        let out = raw.replace(&prefix, "<TMPDIR>");
        insta::with_settings!({snapshot_suffix => name.to_string()}, {
            insta::assert_snapshot!("check_secrets", out);
        });
    }
}

#[test]
fn suggest_dedup_snapshot() {
    let names = fixtures();
    if names.is_empty() {
        return;
    }
    for name in names {
        let tmp = copy_to_tempdir(&name);
        let canonical = tmp.path().canonicalize().expect("canonicalize tmp");
        let prefix = canonical.to_string_lossy().into_owned();
        let raw = run_capture(tmp.path(), &["dedup"]);
        let out = raw.replace(&prefix, "<TMPDIR>");
        insta::with_settings!({snapshot_suffix => name.clone()}, {
            insta::assert_snapshot!("suggest_dedup", out);
        });
    }
}

#[test]
fn suggest_dedup_update_cluster_groups_siblings() {
    let name = "large-estate";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture missing — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let out = run_capture(tmp.path(), &["dedup"]);

    // The text renderer separates clusters with `-- Cluster N` headings. Pick
    // the cluster containing any `update-*.yml` entry and assert it groups at
    // least 2 update-* siblings.
    let target_block = out
        .split("\n\n-- Cluster ")
        .find(|b| b.contains("update-"))
        .unwrap_or_else(|| panic!("no cluster block contains update-*. Full output:\n{out}"));

    let hits = target_block
        .lines()
        .filter(|l| l.contains("update-") && l.contains(".yml"))
        .count();
    assert!(
        hits >= 2,
        "update-* cluster must include at least 2 sibling update-* workflows, got {hits}.\nBlock:\n{target_block}"
    );
}

#[test]
fn suggest_dedup_json_is_valid() {
    let name = "large-estate";
    if !fixtures_dir().join(name).is_dir() {
        eprintln!("{name} fixture missing — skipping");
        return;
    }
    let tmp = copy_to_tempdir(name);
    let stdout = run_capture(tmp.path(), &["dedup", "--format", "json"]);
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = v.as_array().expect("top-level array");
    assert!(
        !arr.is_empty(),
        "expected at least one cluster, got: {stdout}"
    );
    let first = &arr[0];
    for key in [
        "cluster_index",
        "representative",
        "members",
        "common_uses",
        "divergent_uses",
        "triggers_differ",
    ] {
        assert!(
            first.get(key).is_some(),
            "first cluster missing key {key}: {first}"
        );
    }
}
