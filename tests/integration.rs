use assert_cmd::Command;
use globset::GlobSet;
use ravelact::cache::{self, CacheMode};
use ravelact::ir;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::TempDir;

mod common;
use common::{test_cache_path, test_state_dir};

/// Copy `tests/fixtures/simple/` into a fresh tempdir so parallel tests don't
/// collide on cache writes.
fn fresh_simple_fixture() -> TempDir {
    fresh_fixture("simple")
}

fn fresh_fixture(name: &str) -> TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
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
    cmd.arg("--root").arg(root);
    for a in args {
        cmd.arg(a);
    }
    cmd.assert()
}

fn run_with_color_env(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env_remove("NO_COLOR");
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    cmd.arg("--root").arg(root);
    for a in args {
        cmd.arg(a);
    }
    cmd.assert()
}

/// Run `ravelact` with the given args and a piped stdin payload. Used by the
/// stdin / `-` sentinel tests for `impact` and `callers` (issue #75).
fn run_capture_with_stdin(
    root: &Path,
    args: &[&str],
    stdin_input: &str,
) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    cmd.arg("--root").arg(root);
    for a in args {
        cmd.arg(a);
    }
    cmd.write_stdin(stdin_input.to_string());
    cmd.assert()
}

fn wait_for_mtime_tick() {
    std::thread::sleep(Duration::from_secs(1));
}

fn assert_no_text_rhythm_markers(output: &str) {
    for marker in ["-- Summary", "== wf", "x high", "! medium"] {
        assert!(
            !output.contains(marker),
            "non-text output must not contain text rhythm marker {marker:?}: {output}"
        );
    }
}

#[test]
fn build_creates_cache() {
    let tmp = fresh_simple_fixture();
    let root = tmp.path();

    let assert = run(root, &["build"]).success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).to_string();
    assert!(
        stdout.contains("workflows") && stdout.contains("actions"),
        "stdout was: {stdout}"
    );
    assert!(
        stdout.starts_with("build  workflow estate index built\n\nSUMMARY\n"),
        "build stdout should expose status and summary rhythm: {stdout}"
    );
    assert!(
        stdout.contains("metric            value\n"),
        "build stdout should include the summary table header: {stdout}"
    );

    let cache = test_cache_path(root);
    assert!(
        cache.exists(),
        "cache file should exist at {}",
        cache.display()
    );
    assert!(
        !cache.starts_with(root),
        "cache path must live under XDG_STATE_HOME, not the repository root: {}",
        cache.display(),
    );
    let raw = std::fs::read_to_string(&cache).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        v["schema_version"].as_u64().unwrap(),
        u64::from(ir::build::current_schema_version())
    );
    assert_eq!(v["ir"]["workflows"].as_array().unwrap().len(), 4);
    assert_eq!(v["ir"]["actions"].as_array().unwrap().len(), 2);
}

/// `ravelact build` must never write repository-local state. The cache lives
/// under `${XDG_STATE_HOME}/ravelact/repo-<sha8>/cache.json` so adopters
/// do not need to gitignore anything inside their workflow estate.
#[test]
fn build_does_not_write_to_repo_root() {
    let tmp = fresh_simple_fixture();
    let root = tmp.path();

    run(root, &["build"]).success();

    let cache = test_cache_path(root);
    assert!(
        cache.exists(),
        "cache.json should exist under the test XDG state dir at {}",
        cache.display(),
    );
    assert!(
        !cache.starts_with(root),
        "cache path must live under XDG_STATE_HOME, not the repository root: {}",
        cache.display(),
    );
}

/// Same invariant under the `--no-cache` path: even when bypassing reuse, the
/// rebuilt cache lands in the XDG state directory, never inside the repo.
#[test]
fn no_cache_does_not_write_to_repo_root() {
    let tmp = fresh_simple_fixture();
    let root = tmp.path();

    run(root, &["--no-cache", "build"]).success();

    let cache = test_cache_path(root);
    assert!(
        cache.exists(),
        "cache.json should exist under the test XDG state dir at {}",
        cache.display(),
    );
    assert!(
        !cache.starts_with(root),
        "cache path must live under XDG_STATE_HOME, not the repository root: {}",
        cache.display(),
    );
}

#[test]
fn dump_outputs_json() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["dump"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let workflows = v["workflows"].as_array().unwrap();
    assert!(
        workflows
            .iter()
            .any(|w| w["id"].as_str() == Some(".github/workflows/ci.yml")),
        "ci.yml should appear in dump",
    );
}

#[test]
fn incremental_cache_uses_cached_ir_when_valid() {
    let tmp = fresh_simple_fixture();
    let root = tmp.path();

    let first = cache::load_or_build(
        root,
        CacheMode::Default,
        &GlobSet::empty(),
        test_state_dir(),
    )
    .unwrap();
    assert_eq!(first.stats.reparsed_workflows, 4);
    assert_eq!(first.stats.reparsed_actions, 2);
    assert_eq!(first.stats.reused_workflows, 0);
    assert_eq!(first.stats.reused_actions, 0);

    let second = cache::load_or_build(
        root,
        CacheMode::Default,
        &GlobSet::empty(),
        test_state_dir(),
    )
    .unwrap();
    assert_eq!(second.stats.reparsed_workflows, 0);
    assert_eq!(second.stats.reparsed_actions, 0);
    assert_eq!(second.stats.reused_workflows, 4);
    assert_eq!(second.stats.reused_actions, 2);
}

#[test]
fn incremental_cache_reparses_only_edited_workflow() {
    let tmp = fresh_simple_fixture();
    let root = tmp.path();

    cache::load_or_build(
        root,
        CacheMode::Default,
        &GlobSet::empty(),
        test_state_dir(),
    )
    .unwrap();
    wait_for_mtime_tick();

    let workflow = root.join(".github/workflows/build.yml");
    let original = std::fs::read_to_string(&workflow).unwrap();
    std::fs::write(&workflow, format!("{original}\n# touched by test\n")).unwrap();

    let refreshed = cache::load_or_build(
        root,
        CacheMode::Default,
        &GlobSet::empty(),
        test_state_dir(),
    )
    .unwrap();
    assert_eq!(refreshed.stats.reparsed_workflows, 1);
    assert_eq!(refreshed.stats.reused_workflows, 3);
    assert_eq!(refreshed.stats.reparsed_actions, 0);
    assert_eq!(refreshed.stats.reused_actions, 2);
}

#[test]
fn no_cache_forces_full_rebuild() {
    let tmp = fresh_simple_fixture();
    let root = tmp.path();

    cache::load_or_build(
        root,
        CacheMode::Default,
        &GlobSet::empty(),
        test_state_dir(),
    )
    .unwrap();

    let rebuilt = cache::load_or_build(
        root,
        CacheMode::NoCache,
        &GlobSet::empty(),
        test_state_dir(),
    )
    .unwrap();
    assert_eq!(rebuilt.stats.reparsed_workflows, 4);
    assert_eq!(rebuilt.stats.reparsed_actions, 2);
    assert_eq!(rebuilt.stats.reused_workflows, 0);
    assert_eq!(rebuilt.stats.reused_actions, 0);

    let assert = run(root, &["--no-cache", "dump"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert_eq!(v["workflows"].as_array().unwrap().len(), 4);
}

#[test]
fn incremental_cache_detects_deleted_action() {
    let tmp = fresh_simple_fixture();
    let root = tmp.path();

    cache::load_or_build(
        root,
        CacheMode::Default,
        &GlobSet::empty(),
        test_state_dir(),
    )
    .unwrap();
    std::fs::remove_file(root.join(".github/actions/setup/action.yml")).unwrap();

    let refreshed = cache::load_or_build(
        root,
        CacheMode::Default,
        &GlobSet::empty(),
        test_state_dir(),
    )
    .unwrap();
    assert_eq!(refreshed.stats.reparsed_workflows, 0);
    assert_eq!(refreshed.stats.reparsed_actions, 0);
    assert_eq!(refreshed.stats.reused_workflows, 4);
    assert_eq!(refreshed.stats.reused_actions, 1);
    assert!(
        !refreshed
            .ir
            .actions
            .iter()
            .any(|action| action.id.0 == ".github/actions/setup"),
        "deleted local action should not remain in IR"
    );
}

#[test]
fn callers_reverse_lookup() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["callers", ".github/workflows/build.yml"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.starts_with("callers  1 caller for .github/workflows/build.yml\n"),
        "missing per-target summary. stdout was: {stdout}"
    );
    assert!(
        stdout.contains("job-call")
            && stdout.contains(".github/workflows/ci.yml")
            && stdout.contains("call-build::_jobcall"),
        "stdout was: {stdout}"
    );
}

#[test]
fn orphans_detects_unused() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["orphans"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains(".github/workflows/unused.yml"),
        "expected unused.yml in: {stdout}"
    );
    assert!(
        stdout.contains(".github/actions/unused"),
        "expected unused composite in: {stdout}"
    );
    assert!(
        !stdout.contains(".github/actions/setup"),
        "setup composite is used and should not be orphan: {stdout}"
    );
    assert!(
        !stdout.contains(".github/workflows/build.yml"),
        "build.yml is called by ci.yml and should not be orphan: {stdout}"
    );
}

/// Helper for the orphans `--format json` tests below. Returns the parsed
/// JSON object with the four kinds keys (`workflows`, `actions`,
/// `unreferenced_inputs`, `unused_outputs`).
fn run_orphans_json(root: &Path) -> Value {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    let output = cmd
        .arg("--root")
        .arg(root)
        .args(["orphans", "--format", "json"])
        .output()
        .expect("spawn ravelact");
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("non-json orphans output: {e}"))
}

fn json_pair_array_contains(arr: &Value, target: &str, name: &str) -> bool {
    arr.as_array()
        .expect("array")
        .iter()
        .any(|entry| match entry.as_array() {
            Some(items) if items.len() == 2 => {
                items[0].as_str() == Some(target) && items[1].as_str() == Some(name)
            }
            _ => false,
        })
}

#[test]
fn orphans_detects_unreferenced_declared_input() {
    // `dirty` fixture declares `typed-number` on .github/workflows/callee.yml
    // but the callee body never references `${{ inputs.typed-number }}`.
    let tmp = fresh_fixture("dirty");
    let json = run_orphans_json(tmp.path());
    let arr = json
        .get("unreferenced_inputs")
        .expect("unreferenced_inputs key");
    assert!(
        json_pair_array_contains(arr, ".github/workflows/callee.yml", "typed-number"),
        "expected (callee.yml, typed-number) in unreferenced_inputs; got: {json:#}"
    );
}

#[test]
fn orphans_detects_unused_output() {
    // `dirty` fixture declares output `result` on callee.yml; no caller reads
    // it via `needs.<job>.outputs.result`.
    let tmp = fresh_fixture("dirty");
    let json = run_orphans_json(tmp.path());
    let arr = json.get("unused_outputs").expect("unused_outputs key");
    assert!(
        json_pair_array_contains(arr, ".github/workflows/callee.yml", "result"),
        "expected (callee.yml, result) in unused_outputs; got: {json:#}"
    );
}

#[test]
fn orphans_multi_caller_skips_unused_output_when_any_references() {
    // `multi-caller` fixture has at least one caller that consumes the
    // declared output; the migrated semantics must NOT flag it as unused.
    let tmp = fresh_fixture("multi-caller");
    let json = run_orphans_json(tmp.path());
    let arr = json.get("unused_outputs").expect("unused_outputs key");
    let entries = arr.as_array().expect("array");
    assert!(
        entries.is_empty(),
        "expected no unused_outputs when any caller references; got: {json:#}"
    );
    let inputs_arr = json
        .get("unreferenced_inputs")
        .expect("unreferenced_inputs key");
    assert!(
        inputs_arr.as_array().expect("array").is_empty(),
        "expected no unreferenced_inputs in multi-caller; got: {json:#}"
    );
}

#[test]
fn orphans_workflow_dispatch_only_is_excluded() {
    // workflow_dispatch-only workflows expose no `workflow_call` signature, so
    // they are excluded from declared-input / declared-output detection.
    let tmp = fresh_fixture("dispatch-only");
    let json = run_orphans_json(tmp.path());
    let inputs_arr = json
        .get("unreferenced_inputs")
        .expect("unreferenced_inputs key");
    let outputs_arr = json.get("unused_outputs").expect("unused_outputs key");
    assert!(
        inputs_arr.as_array().expect("array").is_empty(),
        "workflow_dispatch-only fixture should produce no unreferenced_inputs; got: {json:#}"
    );
    assert!(
        outputs_arr.as_array().expect("array").is_empty(),
        "workflow_dispatch-only fixture should produce no unused_outputs; got: {json:#}"
    );
}

#[test]
fn orphans_scans_workflow_and_job_env_and_job_if_for_input_refs() {
    // Regression for #115: inputs referenced exclusively from `wf.env`,
    // `job.env`, or `job.if_expr` (i.e. never via a step's `run` / `with` /
    // `env` / `if`) used to be falsely reported as unreferenced because
    // `collect_workflow_expressions` only scanned step-level carriers.
    let tmp = fresh_fixture("synthetic/orphans-job-env-input");
    let json = run_orphans_json(tmp.path());
    let arr = json
        .get("unreferenced_inputs")
        .expect("unreferenced_inputs key");
    let entries = arr.as_array().expect("array");
    assert!(
        entries.is_empty(),
        "expected empty unreferenced_inputs once workflow/job env and job.if_expr are scanned; got: {json:#}"
    );
}

#[test]
fn orphans_js_action_skips_input_scan() {
    // JS / Docker action manifests carry their own runtime that consumes
    // inputs outside YAML; the scan must skip them so declared inputs are
    // not flagged as unreferenced.
    let tmp = fresh_fixture("js-action");
    let json = run_orphans_json(tmp.path());
    let inputs_arr = json
        .get("unreferenced_inputs")
        .expect("unreferenced_inputs key");
    assert!(
        inputs_arr.as_array().expect("array").is_empty(),
        "js-action fixture must not produce unreferenced_inputs; got: {json:#}"
    );
}

#[test]
fn impact_workflow_change_lists_entry_point() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["impact", ".github/workflows/build.yml"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains(".github/workflows/ci.yml"),
        "expected ci.yml in: {stdout}"
    );

    let assert_json = run(
        tmp.path(),
        &["impact", ".github/workflows/build.yml", "--format", "json"],
    )
    .success();
    let stdout_json = String::from_utf8(assert_json.get_output().stdout.clone()).unwrap();
    assert_no_text_rhythm_markers(&stdout_json);
    let v: serde_json::Value = serde_json::from_str(&stdout_json).expect("valid JSON");
    let workflows = v["workflows"].as_array().expect("workflows is array");
    assert!(
        !workflows.is_empty(),
        "workflows array must not be empty: {stdout_json}"
    );
    assert!(
        workflows[0].as_str().is_some(),
        "elements must be bare strings: {stdout_json}"
    );
    assert!(
        workflows
            .iter()
            .any(|x| x.as_str() == Some(".github/workflows/ci.yml")),
        "ci.yml must appear: {stdout_json}"
    );
}

#[test]
fn impact_action_change_lists_entry_point() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["impact", ".github/actions/setup"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains(".github/workflows/ci.yml"),
        "expected ci.yml in: {stdout}"
    );
    assert!(
        !stdout.contains(".github/actions/setup"),
        "seed composite must NOT appear in result: {stdout}"
    );
}

#[test]
fn impact_markdown_renders_workflow_table() {
    let tmp = fresh_simple_fixture();
    let assert = run(
        tmp.path(),
        &[
            "impact",
            ".github/workflows/build.yml",
            "--format",
            "markdown",
        ],
    )
    .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert_no_text_rhythm_markers(&stdout);
    assert!(stdout.contains("### Impact"), "missing heading: {stdout}");
    assert!(
        stdout.contains("| Kind | Target |"),
        "missing table header: {stdout}"
    );
    assert!(
        stdout.contains("| workflow | `.github/workflows/ci.yml` |"),
        "missing ci.yml workflow row: {stdout}"
    );
    assert!(
        !stdout.contains("_No findings._"),
        "non-empty case must not emit empty marker: {stdout}"
    );
}

#[test]
fn impact_markdown_emits_empty_marker_when_no_targets() {
    let tmp = fresh_simple_fixture();
    let assert = run(
        tmp.path(),
        &["impact", "scripts/random.sh", "--format", "markdown"],
    )
    .success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert_no_text_rhythm_markers(&stdout);
    assert!(stdout.contains("### Impact"), "missing heading: {stdout}");
    assert!(
        stdout.contains("No impacted targets found."),
        "missing empty marker: {stdout}"
    );
    assert!(
        !stdout.contains("| Kind | Target |"),
        "empty case must not emit table header: {stdout}"
    );
}

#[test]
fn impact_unknown_path_warns_to_stderr() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["impact", "scripts/random.sh"]).success();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("warn:") && stderr.contains("scripts/random.sh"),
        "expected warn for unknown path in stderr: {stderr}"
    );
}

#[test]
fn impact_transitive_composite_chain_via_cli() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    write_at(
        &root.join(".github/workflows/main.yml"),
        "name: Main\non:\n  push:\n    branches: [main]\njobs:\n  run:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.github/actions/outer\n",
    );
    write_at(
        &root.join(".github/actions/outer/action.yml"),
        "runs:\n  using: composite\n  steps:\n    - uses: ./.github/actions/inner\n",
    );
    write_at(
        &root.join(".github/actions/inner/action.yml"),
        "runs:\n  using: composite\n  steps:\n    - run: echo inner\n      shell: bash\n",
    );

    let assert = run(root, &["impact", ".github/actions/inner"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains(".github/workflows/main.yml"),
        "transitive entry-point main.yml must appear: {stdout}"
    );
    assert!(
        stdout.contains(".github/actions/outer"),
        "intermediate composite outer must appear: {stdout}"
    );
    assert!(
        !stdout.contains(".github/actions/inner"),
        "seed composite inner must NOT appear in result: {stdout}"
    );
}

#[test]
fn graph_snapshot() {
    let tmp = fresh_simple_fixture();
    let out = run(tmp.path(), &["graph"])
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).expect("utf8 stdout");
    assert!(
        out.starts_with("%% generated by ravelact\ngraph LR\n"),
        "mermaid output must lead with header, got:\n{out}"
    );
    insta::assert_snapshot!("graph", out);
}

#[test]
fn graph_event_push_snapshot() {
    let tmp = fresh_simple_fixture();
    let out = run(tmp.path(), &["graph", "--event", "push"])
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).expect("utf8 stdout");
    assert!(
        out.contains("subgraph push\n"),
        "event=push output must include push subgraph, got:\n{out}"
    );
    assert!(
        !out.contains("subgraph pull_request\n"),
        "event=push output must not include pull_request subgraph, got:\n{out}"
    );
    assert!(
        !out.contains("subgraph workflow_dispatch\n"),
        "event=push output must not include workflow_dispatch subgraph, got:\n{out}"
    );
    insta::assert_snapshot!("graph_event_push", out);
}

#[test]
fn graph_markdown_snapshot() {
    let tmp = fresh_simple_fixture();
    let out = run(tmp.path(), &["graph", "--format", "markdown"])
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8(out).expect("utf8 stdout");
    assert!(
        out.starts_with("### Graph\n\n```mermaid\n"),
        "markdown output must lead with `### Graph` heading + fenced mermaid block, got:\n{out}"
    );
    insta::assert_snapshot!("graph_markdown", out);
}

#[test]
fn graph_markdown_event_unmatched() {
    let tmp = fresh_simple_fixture();
    let out = run(
        tmp.path(),
        &[
            "graph",
            "--format",
            "markdown",
            "--event",
            "nonexistent_event_xyz",
        ],
    )
    .success()
    .get_output()
    .stdout
    .clone();
    let out = String::from_utf8(out).expect("utf8 stdout");
    assert!(
        out.contains("%% (no entry-point matches event nonexistent_event_xyz)"),
        "empty-graph markdown output must preserve the diagnostic comment inside the fence, got:\n{out}"
    );
    insta::assert_snapshot!("graph_markdown_event_unmatched", out);
}

#[test]
fn graph_format_json_errors() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["graph", "--format", "json"]).failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    assert!(
        stderr.contains("--format json is not supported for graph"),
        "stderr must mention graph json rejection, got:\n{stderr}"
    );
}

#[test]
fn trace_walks_from_event() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["trace", "push"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains(".github/workflows/ci.yml"),
        "expected ci.yml entry-point: {stdout}"
    );
    assert!(
        stdout.contains(".github/workflows/build.yml"),
        "expected build.yml reachable from ci.yml: {stdout}"
    );
    assert!(
        stdout.contains(".github/actions/setup"),
        "expected setup composite reachable: {stdout}"
    );
    assert!(
        !stdout.contains(".github/workflows/dispatch.yml"),
        "dispatch.yml has no push trigger and should not be in trace: {stdout}"
    );
}

#[test]
fn triggers_summarizes_declared_events() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let workflows = root.join(".github/workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    std::fs::write(
        workflows.join("a.yml"),
        r#"
name: A
on:
  push:
    branches: [main]
  pull_request:
    types: [opened]
  workflow_call:
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo a
"#,
    )
    .unwrap();
    std::fs::write(
        workflows.join("b.yml"),
        r#"
name: B
on:
  push:
    paths: ["src/**"]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo b
"#,
    )
    .unwrap();
    std::fs::write(
        workflows.join("c.yml"),
        r#"
name: C
on:
  workflow_call:
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo c
"#,
    )
    .unwrap();
    std::fs::write(
        workflows.join("future.yml"),
        r#"
name: Future
on:
  future_event:
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo future
"#,
    )
    .unwrap();

    let stdout = String::from_utf8(
        run(root, &["triggers"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(stdout.contains("triggers  4 trigger events"), "{stdout}");
    assert!(
        stdout.contains("event") && stdout.contains("entry workflows"),
        "missing table headers: {stdout}"
    );

    let push_line = stdout
        .lines()
        .find(|line| line.starts_with("push "))
        .unwrap_or_else(|| panic!("missing push row: {stdout}"));
    let push_cells: Vec<&str> = push_line.split_whitespace().collect();
    assert_eq!(&push_cells[0..5], &["push", "2", "2", "0", "2"]);
    assert!(
        push_line.contains(".github/workflows/a.yml, .github/workflows/b.yml"),
        "push examples must be sorted and comma-separated: {push_line}"
    );

    let pr_line = stdout
        .lines()
        .find(|line| line.starts_with("pull_request "))
        .unwrap_or_else(|| panic!("missing pull_request row: {stdout}"));
    let pr_cells: Vec<&str> = pr_line.split_whitespace().collect();
    assert_eq!(&pr_cells[0..5], &["pull_request", "1", "1", "1", "0"]);

    let future_line = stdout
        .lines()
        .find(|line| line.starts_with("future_event "))
        .unwrap_or_else(|| panic!("missing future_event row: {stdout}"));
    let workflow_call_line = stdout
        .lines()
        .find(|line| line.starts_with("workflow_call "))
        .unwrap_or_else(|| panic!("missing workflow_call row: {stdout}"));
    let workflow_call_cells: Vec<&str> = workflow_call_line.split_whitespace().collect();
    assert_eq!(
        &workflow_call_cells[0..5],
        &["workflow_call", "0", "2", "0", "0"]
    );

    let future_idx = stdout.find(future_line).unwrap();
    let pull_request_idx = stdout.find(pr_line).unwrap();
    let workflow_call_idx = stdout.find(workflow_call_line).unwrap();
    assert!(
        future_idx < pull_request_idx && pull_request_idx < workflow_call_idx,
        "tie rows must sort by event name, then workflow_call with zero entries last: {stdout}"
    );
}

#[test]
fn trace_without_event_guides_to_triggers() {
    let tmp = fresh_simple_fixture();
    let assert = run(tmp.path(), &["trace"]).failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).expect("utf8 stderr");
    assert!(
        stderr.contains("`trace` requires a trigger event"),
        "stderr must explain missing trace event: {stderr}"
    );
    assert!(
        stderr.contains("ravelact triggers") && stderr.contains("ravelact trace <event>"),
        "stderr must point to triggers and next trace command: {stderr}"
    );
}

#[test]
fn trace_filters_by_activity_type() {
    // Build a minimal fixture with an `issues: types: [labeled, opened]` entry
    // workflow. `trace issues --type labeled` must hit it; `--type
    // closed` must miss; `--type labeled --type opened` must hit (no double).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wf_path = root.join(".github/workflows/issue-labeled.yml");
    std::fs::create_dir_all(wf_path.parent().unwrap()).unwrap();
    std::fs::write(
        &wf_path,
        r#"
name: Issue Labeled
on:
  issues:
    types: [labeled, opened]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    )
    .unwrap();
    run(root, &["build"]).success();

    let stdout = String::from_utf8(
        run(root, &["trace", "issues", "--type", "labeled"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains(".github/workflows/issue-labeled.yml"),
        "labeled type must hit the workflow: {stdout}"
    );

    let stdout_miss = String::from_utf8(
        run(root, &["trace", "issues", "--type", "closed"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout_miss.contains("trace issues  no entry-point matches  (types=[closed])"),
        "closed must miss explicitly with the type-aware no-match header: {stdout_miss}"
    );

    let stdout_or = String::from_utf8(
        run(
            root,
            &["trace", "issues", "--type", "labeled", "--type", "opened"],
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    let occurrences = stdout_or
        .matches(".github/workflows/issue-labeled.yml")
        .count();
    assert_eq!(
        occurrences, 1,
        "OR-matching must not double-print the same workflow: {stdout_or}"
    );

    // Tree sub-line for matched trigger types.
    assert!(
        stdout.contains("types: labeled, opened"),
        "tree must surface the matched trigger types as a sub-line: {stdout}"
    );

    // Table view embeds the same info in the note column.
    let stdout_table = String::from_utf8(
        run(root, &["trace", "issues", "--view", "table"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout_table.contains("entry, types: labeled, opened"),
        "table note column must embed `entry, types: <list>`: {stdout_table}"
    );
}

#[test]
fn trace_renders_implicit_all_for_issues_without_types() {
    // `on: { issues: }` with `types:` omitted on an event that has no default
    // subset must render `types: any` as a sub-line.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wf_path = root.join(".github/workflows/issue-any.yml");
    std::fs::create_dir_all(wf_path.parent().unwrap()).unwrap();
    std::fs::write(
        &wf_path,
        r#"
name: Issue Any
on:
  issues:
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    )
    .unwrap();
    run(root, &["build"]).success();

    let stdout = String::from_utf8(
        run(root, &["trace", "issues"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains("types: any"),
        "implicit-all (issues without types:) must render `types: any`: {stdout}"
    );
    assert!(
        !stdout.contains("(default)"),
        "implicit-all must not be tagged `(default)`: {stdout}"
    );
}

#[test]
fn trace_renders_implicit_default_for_pull_request_without_types() {
    // `on: { pull_request: }` with `types:` omitted on an event that has a
    // GitHub-defined default subset must render the subset with `(default)`.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wf_path = root.join(".github/workflows/pr.yml");
    std::fs::create_dir_all(wf_path.parent().unwrap()).unwrap();
    std::fs::write(
        &wf_path,
        r#"
name: PR
on:
  pull_request:
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    )
    .unwrap();
    run(root, &["build"]).success();

    let stdout = String::from_utf8(
        run(root, &["trace", "pull_request"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains("types: opened, synchronize, reopened (default)"),
        "implicit-default (pull_request without types:) must render the GitHub default subset with `(default)` tag: {stdout}"
    );
}

#[test]
fn trace_renders_no_sub_line_for_event_without_activity_types() {
    // Events without an activity-type concept (push, schedule, etc.) must NOT
    // emit a sub-line at all — there is nothing meaningful to display.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wf_path = root.join(".github/workflows/ci.yml");
    std::fs::create_dir_all(wf_path.parent().unwrap()).unwrap();
    std::fs::write(
        &wf_path,
        r#"
name: CI
on:
  push:
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    )
    .unwrap();
    run(root, &["build"]).success();

    let stdout = String::from_utf8(
        run(root, &["trace", "push"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains("[wf]"),
        "workflow row must still render with [wf] tag: {stdout}"
    );
    assert!(
        !stdout.contains("types:"),
        "push has no activity-type concept — must not emit any `types:` sub-line: {stdout}"
    );
}

/// Build an inline fixture exercising WF + EX + ANN node kinds in a single
/// trace, used by `--view` tests below.
fn fresh_view_fixture() -> TempDir {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wf_path = root.join(".github/workflows/entry.yml");
    std::fs::create_dir_all(wf_path.parent().unwrap()).unwrap();
    std::fs::write(
        &wf_path,
        r#"
name: Entry
on: push
jobs:
  go:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # ravelact:dispatches .github/workflows/target.yml
      - run: gh workflow run target.yml
"#,
    )
    .unwrap();
    let target_path = root.join(".github/workflows/target.yml");
    std::fs::write(
        &target_path,
        r#"
name: Target
on: workflow_dispatch
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo target
"#,
    )
    .unwrap();
    run(root, &["build"]).success();
    tmp
}

#[test]
fn trace_default_view_is_tree_unicode() {
    let tmp = fresh_simple_fixture();
    let stdout = String::from_utf8(
        run(tmp.path(), &["trace", "push"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains('├') || stdout.contains('└'),
        "default format must include Unicode branch char (├/└): {stdout}"
    );
}

#[test]
fn trace_ascii_falls_back_to_pipe_chars() {
    let tmp = fresh_simple_fixture();
    let stdout = String::from_utf8(
        run(tmp.path(), &["trace", "push", "--ascii"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains("+- ") || stdout.contains("|-> ") || stdout.contains("\\-> "),
        "--ascii must include ASCII connectors (`+- `, `|-> `, `\\-> `): {stdout}"
    );
    assert!(
        !stdout.contains('├') && !stdout.contains('╰') && !stdout.contains('╭'),
        "--ascii must NOT include Unicode branch chars: {stdout}"
    );
}

#[test]
fn trace_no_color_env_suppresses_ansi() {
    // The shared `run` helper already injects NO_COLOR=1, so this asserts the
    // contract end-to-end: stdout must not contain a literal ESC byte (0x1B).
    let tmp = fresh_simple_fixture();
    let stdout = run(tmp.path(), &["trace", "push"])
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        !stdout.contains(&0x1Bu8),
        "stdout under NO_COLOR=1 must contain no ESC byte; got bytes len={} preview={:?}",
        stdout.len(),
        String::from_utf8_lossy(&stdout[..stdout.len().min(120)])
    );
}

#[test]
fn color_always_trace_uses_ansi_when_no_color_is_absent() {
    let tmp = fresh_simple_fixture();
    let stdout = run_with_color_env(tmp.path(), &["--color", "always", "trace", "push"])
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(
        stdout.contains(&0x1Bu8),
        "--color always should emit ANSI when NO_COLOR is absent; preview={:?}",
        String::from_utf8_lossy(&stdout[..stdout.len().min(120)])
    );
}

#[test]
fn color_never_permissions_suppresses_ansi() {
    let tmp = tempfile::tempdir().expect("tempdir");
    write_at(
        &tmp.path().join(".github/workflows/wide.yml"),
        "name: Wide\non: push\npermissions: write-all\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );
    let stdout = run_with_color_env(tmp.path(), &["--color", "never", "permissions"])
        .failure()
        .code(1)
        .get_output()
        .stdout
        .clone();
    assert!(
        !stdout.contains(&0x1Bu8),
        "--color never must suppress ANSI outside trace; preview={:?}",
        String::from_utf8_lossy(&stdout[..stdout.len().min(120)])
    );
}

#[test]
fn trace_view_table_outputs_columns() {
    let tmp = fresh_view_fixture();
    let stdout = String::from_utf8(
        run(tmp.path(), &["trace", "push", "--view", "table"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        !stdout.contains('\u{1b}'),
        "trace table output must not contain ANSI escapes: {stdout}"
    );
    let table = stdout
        .split_once("\n\n")
        .map(|(_, table)| table)
        .unwrap_or(&stdout);
    let mut lines = table.lines();
    assert_eq!(
        lines.next(),
        Some("dep  kind    edge        target                        note"),
        "table header/order changed: {stdout}"
    );
    let rows: Vec<&str> = lines.collect();
    assert!(
        rows.first().is_some_and(|row| {
            row.starts_with("0    wf      entry       .github/workflows/entry.yml")
        }),
        "first row must remain the wf entry row: {stdout}"
    );
    for kind in ["wf", "ext-ac", "ann"] {
        assert!(
            stdout.contains(kind),
            "expected KIND `{kind}` in table body: {stdout}"
        );
    }
    assert_eq!(
        rows.len(),
        4,
        "table body row count must remain stable: {stdout}"
    );
    for (expected_dep, row) in ["0", "1", "1", "2"].iter().zip(rows.iter()) {
        assert!(
            row.starts_with(expected_dep),
            "table rows must remain in traversal order: {stdout}"
        );
    }
}

#[test]
fn trace_view_table_with_ascii_uses_pipe_chars() {
    let tmp = fresh_view_fixture();
    let stdout = String::from_utf8(
        run(tmp.path(), &["trace", "push", "--view", "table", "--ascii"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains("\ndep  kind"),
        "ASCII table must use the plain table header: {stdout}"
    );
    assert!(
        !stdout.contains("┌─"),
        "ASCII table must NOT contain Unicode `┌─`: {stdout}"
    );
}

#[test]
fn trace_empty_result_message_is_view_independent() {
    let tmp = fresh_simple_fixture();
    for view in ["tree", "table"] {
        let stdout = String::from_utf8(
            run(tmp.path(), &["trace", "schedule", "--view", view])
                .success()
                .get_output()
                .stdout
                .clone(),
        )
        .unwrap();
        assert!(
            stdout.contains("trace schedule  no entry-point matches"),
            "view `{view}` did not emit empty-result header: {stdout}"
        );
    }
}

#[test]
fn suggest_extract_finds_4step_bootstrap() {
    let tmp = fresh_fixture("duplicate-steps");
    let assert = run(tmp.path(), &["extract", "--format", "json"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = v.as_array().expect("top-level array");
    assert_eq!(arr.len(), 1, "expected exactly 1 candidate, got: {stdout}");
    let c = &arr[0];
    assert_eq!(c["length"].as_u64(), Some(4), "stdout: {stdout}");
    assert_eq!(
        c["occurrences"].as_array().unwrap().len(),
        3,
        "stdout: {stdout}"
    );
    let containers: BTreeSet<String> = c["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["container"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        containers.len(),
        3,
        "expected 3 distinct containers; got {containers:?}"
    );
    let sketch = c["sketch"].as_str().unwrap();
    assert!(
        sketch.contains("using: composite"),
        "sketch missing composite header: {sketch}"
    );
    assert!(
        sketch.contains("actions/checkout@v4"),
        "sketch missing checkout: {sketch}"
    );
    assert!(sketch.contains("npm ci"), "sketch missing npm ci: {sketch}");
}

#[test]
fn suggest_extract_text_mode_renders_header_and_sketch() {
    let tmp = fresh_fixture("duplicate-steps");
    let assert = run(tmp.path(), &["extract"]).success();
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("CANDIDATE 1"), "stdout: {stdout}");
    assert!(
        stdout.contains("score  length  occurrences"),
        "stdout: {stdout}"
    );
    assert!(stdout.contains("8      4       3"), "stdout: {stdout}");
    assert!(stdout.contains("OCCURRENCES"), "stdout: {stdout}");
    assert!(stdout.contains("SKETCH ACTION.YML"), "stdout: {stdout}");
}

fn run_check_permissions_json(root: &Path) -> (i32, Value) {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    let output = cmd
        .arg("--root")
        .arg(root)
        .args(["permissions", "--format", "json"])
        .output()
        .expect("spawn ravelact");
    let code = output.status.code().expect("exit code");
    let json: Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("non-json output: {e}"));
    (code, json)
}

fn write_at(p: &Path, body: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn check_permissions_overly_broad_write_all() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/wide.yml"),
        "name: Wide\non: push\npermissions: write-all\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(
        code, 1,
        "expected exit 1 on overly-broad fixture; got json: {json:#}"
    );
    let arr = json.as_array().expect("array");
    assert_eq!(arr.len(), 1, "expected exactly one finding; got: {json:#}");
    let f = &arr[0];
    assert_eq!(
        f.get("kind").and_then(Value::as_str),
        Some("OverlyBroadCoarse")
    );
    assert_eq!(f.get("severity").and_then(Value::as_str), Some("high"));
    assert_eq!(
        f.get("workflow").and_then(Value::as_str),
        Some(".github/workflows/wide.yml")
    );
    assert!(f.get("job").map(|v| v.is_null()).unwrap_or(false));
}

#[test]
fn check_permissions_implicit_repo_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/loose.yml"),
        "name: Loose\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo bye\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(code, 1, "expected exit 1; got json: {json:#}");
    let arr = json.as_array().expect("array");
    assert_eq!(arr.len(), 1, "expected exactly one finding; got: {json:#}");
    let f = &arr[0];
    assert_eq!(
        f.get("kind").and_then(Value::as_str),
        Some("ImplicitRepoDefault")
    );
    assert_eq!(f.get("severity").and_then(Value::as_str), Some("medium"));
    assert_eq!(
        f.get("workflow").and_then(Value::as_str),
        Some(".github/workflows/loose.yml")
    );
    let jobs: Vec<&str> = f
        .get("jobs")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert!(
        jobs.contains(&"build") && jobs.contains(&"test"),
        "expected both jobs in `jobs`; got: {jobs:?}"
    );
}

/// Workflows that explicitly declare `permissions: {}` are deliberate
/// hardenings, not implicit repo-default cases — (c) must not fire.
#[test]
fn check_permissions_empty_map_is_explicit_not_implicit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/locked.yml"),
        "name: Locked\non: push\npermissions: {}\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(code, 0, "expected exit 0 (clean); got json: {json:#}");
    assert!(json.as_array().unwrap().is_empty());
}

#[test]
fn check_permissions_callee_escalates() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Caller: push trigger, job-level `contents: read` (no workflow-level perms).
    write_at(
        &root.join(".github/workflows/caller.yml"),
        "name: Caller\non: push\njobs:\n  call:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/callee.yml\n",
    );
    // Callee: workflow_call, job-level `contents: write` (escalation).
    write_at(
        &root.join(".github/workflows/callee.yml"),
        "name: Callee\non:\n  workflow_call:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(code, 1, "expected exit 1; got json: {json:#}");
    let arr = json.as_array().expect("array");
    let escalations: Vec<&serde_json::Value> = arr
        .iter()
        .filter(|f| f.get("kind").and_then(Value::as_str) == Some("CalleeEscalatesCaller"))
        .collect();
    assert_eq!(
        escalations.len(),
        1,
        "expected exactly one CalleeEscalatesCaller finding; got: {json:#}"
    );
    let f = escalations[0];
    assert_eq!(f.get("severity").and_then(Value::as_str), Some("high"));
    let scopes: Vec<&str> = f
        .get("scopes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert!(
        scopes.contains(&"contents"),
        "expected `contents` in escalated scopes; got: {scopes:?}"
    );
    assert!(
        f.get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .contains("contents"),
        "message should mention escalated scope; got: {f:#}"
    );

    // Chain captures the entry-point job only (1-hop direct caller→callee).
    let chain = f
        .get("chain")
        .and_then(Value::as_array)
        .expect("chain field present and array");
    assert_eq!(
        chain.len(),
        1,
        "expected chain.len() == 1 for 1-hop; got: {chain:#?}"
    );
    assert_eq!(
        chain[0].get("workflow").and_then(Value::as_str),
        Some(".github/workflows/caller.yml")
    );
    assert_eq!(chain[0].get("job").and_then(Value::as_str), Some("call"));
}

/// Issue #88 Acceptance #1: a 2-hop chain where A is a pure passthrough — no
/// permissions at any layer. Today's 1-hop checker skips both hops (entry↔A
/// because callee_decl is None, A↔B because caller_eff is None for `mid`),
/// so the chain silently elides. Only the transitive walk, which carries
/// entry's `contents: read` cap unchanged through A, catches B's
/// `contents: write` declaration.
#[test]
fn check_permissions_two_hop_hidden_by_intermediate() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Entry: push trigger, job-level contents: read (cap = read).
    write_at(
        &root.join(".github/workflows/entry.yml"),
        "name: Entry\non: push\njobs:\n  start:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/a.yml\n",
    );
    // A: passthrough — no workflow-level perms, no job-level perms on `mid`.
    // Today's 1-hop checker has nothing to compare here.
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non:\n  workflow_call:\njobs:\n  mid:\n    uses: ./.github/workflows/b.yml\n",
    );
    // B: reusable leaf with contents: write — escalates against entry's read
    // cap, but old 1-hop never sees the chain because A elides.
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\njobs:\n  leaf:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(code, 1, "expected exit 1; got json: {json:#}");
    let arr = json.as_array().expect("array");
    let escalations: Vec<&serde_json::Value> = arr
        .iter()
        .filter(|f| {
            f.get("kind").and_then(Value::as_str) == Some("CalleeEscalatesCaller")
                && f.get("callee_job").and_then(Value::as_str) == Some("leaf")
        })
        .collect();
    assert_eq!(
        escalations.len(),
        1,
        "expected exactly one escalation at leaf; got: {json:#}"
    );
    let f = escalations[0];
    let chain = f
        .get("chain")
        .and_then(Value::as_array)
        .expect("chain field present and array");
    assert_eq!(
        chain.len(),
        2,
        "expected chain.len() == 2 (entry-job, mid-job); got: {chain:#?}"
    );
    assert_eq!(
        chain[0].get("workflow").and_then(Value::as_str),
        Some(".github/workflows/entry.yml")
    );
    assert_eq!(chain[0].get("job").and_then(Value::as_str), Some("start"));
    assert_eq!(
        chain[1].get("workflow").and_then(Value::as_str),
        Some(".github/workflows/a.yml")
    );
    assert_eq!(chain[1].get("job").and_then(Value::as_str), Some("mid"));
    let scopes: Vec<&str> = f
        .get("scopes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert!(scopes.contains(&"contents"), "got: {scopes:?}");
}

/// Issue #88 Acceptance #2: a 3-hop chain that monotonically narrows or stays
/// equal at every hop. The transitive walk must NOT emit any escalation.
#[test]
fn check_permissions_three_hop_clean() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/entry.yml"),
        "name: Entry\non: push\njobs:\n  start:\n    permissions:\n      contents: write\n    uses: ./.github/workflows/a.yml\n",
    );
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non:\n  workflow_call:\njobs:\n  mid:\n    permissions:\n      contents: write\n    uses: ./.github/workflows/b.yml\n",
    );
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\njobs:\n  inner:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/c.yml\n",
    );
    write_at(
        &root.join(".github/workflows/c.yml"),
        "name: C\non:\n  workflow_call:\njobs:\n  leaf:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: read\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(
        code, 0,
        "expected exit 0 (no escalations); got json: {json:#}"
    );
    let escalations: Vec<&serde_json::Value> = json
        .as_array()
        .expect("array")
        .iter()
        .filter(|f| f.get("kind").and_then(Value::as_str) == Some("CalleeEscalatesCaller"))
        .collect();
    assert!(
        escalations.is_empty(),
        "expected zero CalleeEscalatesCaller findings; got: {json:#}"
    );
}

/// A 3-hop chain where the leaf escalates against the entry cap. The
/// intermediate hops do not themselves declare anything that would make a
/// local-only check fire at the entry↔A or A↔B boundary.
#[test]
fn check_permissions_three_hop_hidden() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Entry cap = contents: read.
    write_at(
        &root.join(".github/workflows/entry.yml"),
        "name: Entry\non: push\njobs:\n  start:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/a.yml\n",
    );
    // A and B inherit silently (no perms declared anywhere) — the cap
    // propagates through them unchanged. The old 1-hop checker skips both
    // hops because the callee declarations are absent.
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non:\n  workflow_call:\njobs:\n  mid:\n    uses: ./.github/workflows/b.yml\n",
    );
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\njobs:\n  inner:\n    uses: ./.github/workflows/c.yml\n",
    );
    // C escalates: declares contents: write at job level.
    write_at(
        &root.join(".github/workflows/c.yml"),
        "name: C\non:\n  workflow_call:\njobs:\n  leaf:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(code, 1, "expected exit 1; got json: {json:#}");
    let escalations: Vec<&serde_json::Value> = json
        .as_array()
        .expect("array")
        .iter()
        .filter(|f| {
            f.get("kind").and_then(Value::as_str) == Some("CalleeEscalatesCaller")
                && f.get("callee_job").and_then(Value::as_str) == Some("leaf")
        })
        .collect();
    assert_eq!(
        escalations.len(),
        1,
        "expected exactly one escalation at leaf; got: {json:#}"
    );
    let chain = escalations[0]
        .get("chain")
        .and_then(Value::as_array)
        .expect("chain present");
    assert_eq!(
        chain.len(),
        3,
        "expected chain.len() == 3 (entry, A, B caller-side); got: {chain:#?}"
    );
}

/// Issue #88 Acceptance #3: `secrets:` propagation differences (`inherit` vs
/// explicit map vs absent) must not alter permission semantics. Two parallel
/// chains with identical permissions but different secrets handling produce
/// the same set of permissions findings.
#[test]
fn check_permissions_secrets_inherit_does_not_alter_cap() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Chain X: secrets: inherit at every hop.
    write_at(
        &root.join(".github/workflows/entry-x.yml"),
        "name: EntryX\non: push\njobs:\n  start:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/x.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/x.yml"),
        "name: X\non:\n  workflow_call:\njobs:\n  leaf:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write\n    steps:\n      - run: echo hi\n",
    );
    // Chain Y: no secrets clause anywhere, identical permissions structure.
    write_at(
        &root.join(".github/workflows/entry-y.yml"),
        "name: EntryY\non: push\njobs:\n  start:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/y.yml\n",
    );
    write_at(
        &root.join(".github/workflows/y.yml"),
        "name: Y\non:\n  workflow_call:\njobs:\n  leaf:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: write\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(code, 1, "expected exit 1; got json: {json:#}");
    let escalations: Vec<&serde_json::Value> = json
        .as_array()
        .expect("array")
        .iter()
        .filter(|f| f.get("kind").and_then(Value::as_str) == Some("CalleeEscalatesCaller"))
        .collect();
    // Two parallel chains, both should escalate identically.
    assert_eq!(
        escalations.len(),
        2,
        "expected one escalation per chain; got: {json:#}"
    );
    let by_callee: std::collections::BTreeMap<&str, &serde_json::Value> = escalations
        .iter()
        .filter_map(|f| f.get("callee").and_then(Value::as_str).map(|c| (c, *f)))
        .collect();
    let x = by_callee
        .get(".github/workflows/x.yml")
        .expect("X chain finding");
    let y = by_callee
        .get(".github/workflows/y.yml")
        .expect("Y chain finding");
    let chain_len = |v: &serde_json::Value| {
        v.get("chain")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0)
    };
    assert_eq!(
        chain_len(x),
        chain_len(y),
        "chain lengths must match between secrets-inherit and no-secrets chains"
    );
    assert_eq!(
        x.get("scopes").and_then(Value::as_array),
        y.get("scopes").and_then(Value::as_array),
        "scopes must be identical between the two chains"
    );
}

/// A workflow_call cycle (A → B → A) must terminate analysis without panic
/// or unbounded recursion. The cycle guard is `visiting: BTreeSet<String>`.
#[test]
fn check_permissions_cycle_does_not_loop() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/entry.yml"),
        "name: Entry\non: push\njobs:\n  start:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/a.yml\n",
    );
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non:\n  workflow_call:\njobs:\n  hop:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/b.yml\n",
    );
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\njobs:\n  hop:\n    permissions:\n      contents: read\n    uses: ./.github/workflows/a.yml\n",
    );

    // Termination is the primary contract. Also pin the dedup contract: a
    // cycled chain with no real escalation must not emit any
    // CalleeEscalatesCaller finding (guard against double-emit on revisit).
    let (code, json) = run_check_permissions_json(root);
    assert_eq!(code, 0, "cycle chain has no escalations and must exit 0");
    let escalations: Vec<&serde_json::Value> = json
        .as_array()
        .expect("array")
        .iter()
        .filter(|f| f.get("kind").and_then(Value::as_str) == Some("CalleeEscalatesCaller"))
        .collect();
    assert!(
        escalations.is_empty(),
        "cycle must not emit duplicate escalations; got: {json:#}"
    );
}

/// `permissions: {}` is an explicit empty Scopes map — it caps every scope at
/// `Level::None`. Any callee that declares any scope above `none` escalates.
#[test]
fn check_permissions_empty_scopes_caps_callee_to_none() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Entry has workflow-level `permissions: {}` (all-none cap).
    write_at(
        &root.join(".github/workflows/entry.yml"),
        "name: Entry\non: push\npermissions: {}\njobs:\n  start:\n    uses: ./.github/workflows/leaf.yml\n",
    );
    write_at(
        &root.join(".github/workflows/leaf.yml"),
        "name: Leaf\non:\n  workflow_call:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    permissions:\n      contents: read\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(code, 1, "expected exit 1; got json: {json:#}");
    let escalations: Vec<&serde_json::Value> = json
        .as_array()
        .expect("array")
        .iter()
        .filter(|f| f.get("kind").and_then(Value::as_str) == Some("CalleeEscalatesCaller"))
        .collect();
    assert_eq!(
        escalations.len(),
        1,
        "expected one escalation against empty-scopes cap; got: {json:#}"
    );
    let scopes: Vec<&str> = escalations[0]
        .get("scopes")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    assert!(
        scopes.contains(&"contents"),
        "expected `contents` in escalated scopes; got: {scopes:?}"
    );
}

/// Reusable-only workflows (only `workflow_call` triggers) are skipped for
/// (c) — they receive their permissions cap from the caller, not the repo
/// default.
#[test]
fn check_permissions_reusable_only_skipped_for_implicit() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/lib.yml"),
        "on:\n  workflow_call:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_permissions_json(root);
    assert_eq!(
        code, 0,
        "expected exit 0 (reusable-only skipped); got: {json:#}"
    );
    assert!(json.as_array().unwrap().is_empty());
}

/// A workflow with `permissions: typo-value` (unknown coarse string) must
/// produce a `ParseDiagnostic` instead of silently normalizing to an empty
/// scope map. The diagnostic surfaces to callers via `RebuildResult.diagnostics`.
#[test]
fn unknown_coarse_permissions_emits_diagnostic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/typo.yml"),
        "name: Typo\non: push\npermissions: read-al\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );

    let inventory = ir::build::discover_sources(root, &GlobSet::empty()).expect("discover");
    let result = ir::build::rebuild_ir_from_inventory(&inventory, None, &Default::default())
        .expect("rebuild");

    assert_eq!(
        result.diagnostics.len(),
        1,
        "expected exactly one diagnostic for unknown coarse permissions; got: {:?}",
        result.diagnostics
    );
    let diag = &result.diagnostics[0];
    assert!(
        diag.message.contains("read-al"),
        "diagnostic message should mention the bad value; got: {}",
        diag.message
    );

    // The IR should still have a Permissions node (Unknown variant), not None.
    let wf = result
        .ir
        .workflows
        .iter()
        .find(|w| w.id.0 == ".github/workflows/typo.yml")
        .expect("workflow in IR");
    assert!(
        wf.permissions.is_some(),
        "permissions field must be Some(Coarse(Unknown(..))) not None"
    );
}

/// A workflow with an unknown scope key emits a diagnostic for each unknown
/// key but still builds the IR node.
#[test]
fn unknown_scope_key_emits_diagnostic() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/scope.yml"),
        "name: Scope\non: push\npermissions:\n  contents: read\n  future-scope: write\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );

    let inventory = ir::build::discover_sources(root, &GlobSet::empty()).expect("discover");
    let result = ir::build::rebuild_ir_from_inventory(&inventory, None, &Default::default())
        .expect("rebuild");

    assert_eq!(
        result.diagnostics.len(),
        1,
        "expected one diagnostic for the unknown scope key; got: {:?}",
        result.diagnostics
    );
    let diag = &result.diagnostics[0];
    assert!(
        diag.message.contains("future-scope"),
        "diagnostic message should name the unknown key; got: {}",
        diag.message
    );
}

// ---------------------------------------------------------------------------
// check secrets — Phase 3 propagation analyzer
// ---------------------------------------------------------------------------

fn run_check_secrets_json(root: &Path) -> (i32, Value) {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    let output = cmd
        .arg("--root")
        .arg(root)
        .args(["secrets", "--format", "json"])
        .output()
        .expect("spawn ravelact");
    let code = output.status.code().expect("exit code");
    let json: Value =
        serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("non-json output: {e}"));
    (code, json)
}

fn check_secrets_kinds(json: &Value) -> Vec<String> {
    json.as_array()
        .expect("array")
        .iter()
        .filter_map(|f| f.get("kind").and_then(Value::as_str).map(String::from))
        .collect()
}

#[test]
fn check_secrets_clean_no_callees() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/standalone.yml"),
        "name: Solo\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 0, "expected clean: {json:#}");
    assert!(json.as_array().unwrap().is_empty());
}

#[test]
fn check_secrets_missing_propagation_depth1() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Caller does not declare `secrets:` -> SecretsPass::None
    write_at(
        &root.join(".github/workflows/caller.yml"),
        "name: Caller\non: push\njobs:\n  call:\n    uses: ./.github/workflows/callee.yml\n",
    );
    write_at(
        &root.join(".github/workflows/callee.yml"),
        "name: Callee\non:\n  workflow_call:\n    secrets:\n      DEPLOY_TOKEN:\n        required: true\njobs:\n  do:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 1, "expected exit 1: {json:#}");
    let arr = json.as_array().expect("array");
    assert_eq!(arr.len(), 1, "expected 1 finding: {json:#}");
    let f = &arr[0];
    assert_eq!(
        f.get("kind").and_then(Value::as_str),
        Some("MissingSecretPropagation")
    );
    assert_eq!(f.get("severity").and_then(Value::as_str), Some("high"));
    assert_eq!(
        f.get("secret").and_then(Value::as_str),
        Some("DEPLOY_TOKEN")
    );
    assert_eq!(
        f.get("caller").and_then(Value::as_str),
        Some(".github/workflows/caller.yml")
    );
    assert_eq!(
        f.get("callee").and_then(Value::as_str),
        Some(".github/workflows/callee.yml")
    );

    let text = run(root, &["secrets"]).failure();
    let stdout = String::from_utf8(text.get_output().stdout.clone()).expect("utf8 stdout");
    assert!(
        stdout.contains("X  high  missing-secret-propagation"),
        "secrets text output should expose the high-severity finding rhythm: {stdout}"
    );
    assert!(
        stdout.contains("  .github/workflows/caller.yml:call"),
        "secrets text output should keep the path on its own line: {stdout}"
    );
}

#[test]
fn check_secrets_explicit_empty_drops_all() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // `secrets: {}` is parsed as Explicit(empty) -> drops everything.
    write_at(
        &root.join(".github/workflows/caller.yml"),
        "name: Caller\non: push\njobs:\n  call:\n    uses: ./.github/workflows/callee.yml\n    secrets: {}\n",
    );
    write_at(
        &root.join(".github/workflows/callee.yml"),
        "name: Callee\non:\n  workflow_call:\n    secrets:\n      DEPLOY_TOKEN:\n        required: true\njobs:\n  do:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 1);
    let arr = json.as_array().expect("array");
    let kinds = check_secrets_kinds(&json);
    assert!(
        kinds.contains(&"MissingSecretPropagation".to_string()),
        "expected MissingSecretPropagation in {kinds:?}; full: {arr:#?}"
    );
}

#[test]
fn check_secrets_chain_break_mid_drops() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // A inherit-> B; B drops to C; C requires DEPLOY_TOKEN.
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non: push\njobs:\n  call:\n    uses: ./.github/workflows/b.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\njobs:\n  forward:\n    uses: ./.github/workflows/c.yml\n    secrets: {}\n",
    );
    write_at(
        &root.join(".github/workflows/c.yml"),
        "name: C\non:\n  workflow_call:\n    secrets:\n      DEPLOY_TOKEN:\n        required: true\njobs:\n  consume:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );

    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 1, "expected exit 1: {json:#}");
    let arr = json.as_array().expect("array");
    let kinds = check_secrets_kinds(&json);
    assert!(
        kinds.contains(&"SecretsInheritChainBreak".to_string()),
        "expected SecretsInheritChainBreak in {kinds:?}; full: {arr:#?}"
    );
    assert!(
        !kinds.contains(&"MissingSecretPropagation".to_string()),
        "should not double-emit MissingSecretPropagation for the same secret; kinds={kinds:?}"
    );
    let chain_break = arr
        .iter()
        .find(|f| f.get("kind").and_then(Value::as_str) == Some("SecretsInheritChainBreak"))
        .expect("chain-break finding");
    assert_eq!(
        chain_break.get("dropped_at").and_then(Value::as_str),
        Some(".github/workflows/b.yml"),
        "B dropped the secret; got: {chain_break:#?}"
    );
    assert_eq!(
        chain_break.get("secret").and_then(Value::as_str),
        Some("DEPLOY_TOKEN")
    );
}

#[test]
fn check_secrets_chain_break_mid_renames() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // B passes `OTHER` instead of forwarding the requested DEPLOY_TOKEN.
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non: push\njobs:\n  call:\n    uses: ./.github/workflows/b.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\njobs:\n  forward:\n    uses: ./.github/workflows/c.yml\n    secrets:\n      OTHER: ${{ secrets.OTHER }}\n",
    );
    write_at(
        &root.join(".github/workflows/c.yml"),
        "name: C\non:\n  workflow_call:\n    secrets:\n      DEPLOY_TOKEN:\n        required: true\njobs:\n  consume:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 1);
    let kinds = check_secrets_kinds(&json);
    assert!(
        kinds.contains(&"SecretsInheritChainBreak".to_string()),
        "expected chain-break: {kinds:?}; json={json:#?}"
    );
}

#[test]
fn check_secrets_chain_break_inherit_subset() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // A passes only OTHER explicitly to B; B inherits forward to C; C requires DEPLOY_TOKEN.
    // B's `inherit` only carries A's narrowed set ({OTHER}), so DEPLOY_TOKEN never reaches C.
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non: push\njobs:\n  call:\n    uses: ./.github/workflows/b.yml\n    secrets:\n      OTHER: ${{ secrets.OTHER }}\n",
    );
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\n    secrets:\n      OTHER:\n        required: false\njobs:\n  forward:\n    uses: ./.github/workflows/c.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/c.yml"),
        "name: C\non:\n  workflow_call:\n    secrets:\n      DEPLOY_TOKEN:\n        required: true\njobs:\n  consume:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 1);
    let arr = json.as_array().expect("array");
    let chain_break = arr
        .iter()
        .find(|f| f.get("kind").and_then(Value::as_str) == Some("SecretsInheritChainBreak"))
        .unwrap_or_else(|| panic!("expected chain-break; got: {json:#?}"));
    assert_eq!(
        chain_break.get("dropped_at").and_then(Value::as_str),
        Some(".github/workflows/a.yml"),
        "A is the layer that narrowed the reachable set; got: {chain_break:#?}"
    );
}

#[test]
fn check_secrets_inherit_chain_clean() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non: push\njobs:\n  call:\n    uses: ./.github/workflows/b.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\njobs:\n  forward:\n    uses: ./.github/workflows/c.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/c.yml"),
        "name: C\non:\n  workflow_call:\n    secrets:\n      DEPLOY_TOKEN:\n        required: true\njobs:\n  consume:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 0, "all-inherit chain should be clean: {json:#}");
    assert!(json.as_array().unwrap().is_empty());
}

#[test]
fn check_secrets_environment_shadow() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/caller.yml"),
        "name: Caller\non: push\njobs:\n  call:\n    uses: ./.github/workflows/callee.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/callee.yml"),
        "name: Callee\non:\n  workflow_call:\njobs:\n  deploy:\n    runs-on: ubuntu-latest\n    environment: prod\n    steps:\n      - run: echo hi\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 1, "expected exit 1: {json:#}");
    let arr = json.as_array().expect("array");
    let env_finding = arr
        .iter()
        .find(|f| f.get("kind").and_then(Value::as_str) == Some("EnvironmentInWorkflowCallCallee"))
        .unwrap_or_else(|| panic!("expected env shadow finding; got: {json:#?}"));
    assert_eq!(
        env_finding.get("severity").and_then(Value::as_str),
        Some("medium")
    );
    assert_eq!(
        env_finding.get("environment").and_then(Value::as_str),
        Some("prod")
    );
    assert_eq!(
        env_finding.get("workflow").and_then(Value::as_str),
        Some(".github/workflows/callee.yml")
    );
}

#[test]
fn check_secrets_environment_isolated_reusable_skipped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // Reusable workflow with environment: prod, but nothing references it.
    write_at(
        &root.join(".github/workflows/lib.yml"),
        "name: Lib\non:\n  workflow_call:\njobs:\n  deploy:\n    runs-on: ubuntu-latest\n    environment: prod\n    steps:\n      - run: echo hi\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(
        code, 0,
        "isolated reusable should not trigger env shadow finding: {json:#}"
    );
    assert!(json.as_array().unwrap().is_empty());
}

#[test]
fn check_secrets_external_callee_opaque() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/caller.yml"),
        "name: Caller\non: push\njobs:\n  call:\n    uses: acme/foo/.github/workflows/x.yml@v1\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 0, "external callee opaque -> clean: {json:#}");
    assert!(json.as_array().unwrap().is_empty());
}

#[test]
fn check_secrets_explicit_chain_clean() {
    // Synthetic fixture: depth=2 explicit-map propagation chain (caller -> mid -> leaf).
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/synthetic/secrets-explicit-chain");
    let tmp = tempfile::tempdir().expect("tempdir");
    copy_tree(&fixture, tmp.path());
    let (code, json) = run_check_secrets_json(tmp.path());
    assert_eq!(code, 0, "explicit chain should be clean: {json:#}");
    assert!(json.as_array().unwrap().is_empty());
}

#[test]
fn check_secrets_self_loop_does_not_hang() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    // self.yml has both push (entry-point) and workflow_call, and calls itself
    // via uses: ./.github/workflows/self.yml. DFS must terminate via cycle guard.
    write_at(
        &root.join(".github/workflows/self.yml"),
        "name: Self\non:\n  push:\n  workflow_call:\njobs:\n  recurse:\n    uses: ./.github/workflows/self.yml\n    secrets: inherit\n",
    );
    // Use std::process to bound wall-clock with a timeout; assert_cmd lacks one.
    let started = std::time::Instant::now();
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    let output = cmd
        .arg("--root")
        .arg(root)
        .args(["secrets", "--format", "json"])
        .timeout(Duration::from_secs(5))
        .output()
        .expect("spawn");
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "check secrets must terminate quickly on a self-loop; took {elapsed:?}"
    );
    let _: Value = serde_json::from_slice(&output.stdout).expect("parses JSON");
}

#[test]
fn check_secrets_diamond_partial_drop() {
    // A.jobX -> B (drops) -> D (requires X)
    // A.jobY -> C (inherit) -> D
    // Expected: chain-break finding for the B path; C path is clean.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    write_at(
        &root.join(".github/workflows/a.yml"),
        "name: A\non: push\njobs:\n  via_b:\n    uses: ./.github/workflows/b.yml\n    secrets: inherit\n  via_c:\n    uses: ./.github/workflows/c.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/b.yml"),
        "name: B\non:\n  workflow_call:\njobs:\n  forward:\n    uses: ./.github/workflows/d.yml\n    secrets: {}\n",
    );
    write_at(
        &root.join(".github/workflows/c.yml"),
        "name: C\non:\n  workflow_call:\njobs:\n  forward:\n    uses: ./.github/workflows/d.yml\n    secrets: inherit\n",
    );
    write_at(
        &root.join(".github/workflows/d.yml"),
        "name: D\non:\n  workflow_call:\n    secrets:\n      DEPLOY_TOKEN:\n        required: true\njobs:\n  consume:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );
    let (code, json) = run_check_secrets_json(root);
    assert_eq!(code, 1, "expected exit 1: {json:#}");
    let arr = json.as_array().expect("array");
    let chain_breaks: Vec<&Value> = arr
        .iter()
        .filter(|f| f.get("kind").and_then(Value::as_str) == Some("SecretsInheritChainBreak"))
        .collect();
    assert_eq!(
        chain_breaks.len(),
        1,
        "exactly one chain-break (B path); got: {arr:#?}"
    );
    let chain = &chain_breaks[0];
    assert_eq!(
        chain.get("dropped_at").and_then(Value::as_str),
        Some(".github/workflows/b.yml")
    );
    assert_eq!(
        chain.get("secret").and_then(Value::as_str),
        Some("DEPLOY_TOKEN")
    );
}

// -- New JSON output tests for callers / orphans / wiring ------------------

#[test]
fn callers_json_output() {
    let tmp = fresh_simple_fixture();

    // build.yml is invoked at job level by ci.yml — JobCall variant.
    let stdout = String::from_utf8(
        run(
            tmp.path(),
            &["callers", ".github/workflows/build.yml", "--format", "json"],
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = v.as_array().expect("top-level array");
    // Single positional → length-1 array (uniform shape).
    assert_eq!(arr.len(), 1, "expected length-1 array, got: {stdout}");
    let entry = &arr[0];
    assert_eq!(
        entry.get("target").and_then(Value::as_str),
        Some(".github/workflows/build.yml"),
        "target field must echo the user input verbatim: {stdout}"
    );
    let hits = entry
        .get("hits")
        .and_then(Value::as_array)
        .expect("hits is array");
    assert!(!hits.is_empty(), "expected callers: {stdout}");
    let kinds: BTreeSet<String> = hits
        .iter()
        .filter_map(|e| e.get("kind").and_then(Value::as_str).map(String::from))
        .collect();
    assert!(
        kinds.contains("JobCall"),
        "expected JobCall variant in: {kinds:?}"
    );
    let job_call = hits
        .iter()
        .find(|e| e.get("kind").and_then(Value::as_str) == Some("JobCall"))
        .expect("JobCall entry");
    assert_eq!(
        job_call.get("workflow").and_then(Value::as_str),
        Some(".github/workflows/ci.yml")
    );
    assert!(job_call.get("job").is_some(), "JobCall needs job field");

    // .github/actions/setup is used at step level — Step variant for the workflow caller.
    let stdout2 = String::from_utf8(
        run(
            tmp.path(),
            &["callers", ".github/actions/setup", "--format", "json"],
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    let v2: Value = serde_json::from_str(&stdout2).expect("valid JSON");
    let arr2 = v2.as_array().expect("top-level array");
    assert_eq!(arr2.len(), 1, "expected length-1 array, got: {stdout2}");
    let hits2 = arr2[0]
        .get("hits")
        .and_then(Value::as_array)
        .expect("hits is array");
    let kinds2: BTreeSet<String> = hits2
        .iter()
        .filter_map(|e| e.get("kind").and_then(Value::as_str).map(String::from))
        .collect();
    assert!(
        kinds2.contains("Step") || kinds2.contains("CompositeStep"),
        "expected Step or CompositeStep variant in: {kinds2:?}"
    );
}

/// Issue #83: `callers --format text` appends `  (name: "...")` after the
/// step locator when the step has a `name:` field. Covers Step (named /
/// unnamed / multi-line) and CompositeStep variants. `Annotated::Step` is
/// exercised separately in `tests/annotations.rs`.
#[test]
fn callers_text_includes_step_name_suffix() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Workflow with three step-level uses of `inner`:
    //   index 0: named "Setup"               -> Step variant + suffix
    //   index 1: unnamed                     -> Step variant, NO suffix
    //   index 2: multi-line block-scalar name -> suffix with escaped \n
    write_at(
        &root.join(".github/workflows/wf.yaml"),
        "name: wf\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - name: Setup\n        uses: ./.github/actions/inner\n      - uses: ./.github/actions/inner\n      - name: |\n          first\n          second\n        uses: ./.github/actions/inner\n",
    );
    // Composite `inner` references composite `leaf` from a named step ->
    // CompositeStep variant + suffix.
    write_at(
        &root.join(".github/actions/inner/action.yml"),
        "name: inner\ndescription: nests leaf\nruns:\n  using: composite\n  steps:\n    - name: Run nested\n      uses: ./.github/actions/leaf\n",
    );
    // Composite `leaf` is a trivial run-only step (no further `uses:`).
    write_at(
        &root.join(".github/actions/leaf/action.yml"),
        "name: leaf\ndescription: trivial\nruns:\n  using: composite\n  steps:\n    - run: ':'\n      shell: bash\n",
    );

    // ---- callers .github/actions/inner -> three Step hits in wf.yaml ----
    let stdout_inner = String::from_utf8(
        run(root, &["callers", ".github/actions/inner"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();

    let lines_inner: Vec<&str> = stdout_inner.lines().collect();
    let step_lines: Vec<&&str> = lines_inner
        .iter()
        .filter(|l| l.starts_with("step  .github/workflows/wf.yaml"))
        .collect();
    assert_eq!(
        step_lines.len(),
        3,
        "expected 3 Step caller lines in: {stdout_inner}"
    );

    // (i) named "Setup" step -> suffix appended.
    assert!(
        step_lines
            .iter()
            .any(|l| l.contains("build:0") && l.contains("name=\"Setup\"")),
        "expected Step variant name detail in: {stdout_inner}"
    );

    // (ii) unnamed step -> no `(name:` suffix at all.
    assert!(
        step_lines
            .iter()
            .any(|l| l.contains("build:1") && !l.contains("name=")),
        "expected unnamed Step line WITHOUT suffix in: {stdout_inner}"
    );

    // (iii) multi-line block-scalar name -> serde_json escapes `\n` so the
    // entire suffix stays on one line.
    assert!(
        step_lines
            .iter()
            .any(|l| l.contains("build:2") && l.contains("name=\"first\\nsecond\\n\"")),
        "expected multi-line name escaped to one line in: {stdout_inner}"
    );

    // ---- callers .github/actions/leaf -> CompositeStep hit in inner ----
    let stdout_leaf = String::from_utf8(
        run(root, &["callers", ".github/actions/leaf"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout_leaf
            .lines()
            .any(|l| l.starts_with("composite-step  .github/actions/inner")
                && l.contains("name=\"Run nested\"")),
        "expected CompositeStep variant name detail in: {stdout_leaf}"
    );
}

#[test]
fn orphans_json_output() {
    // The simple fixture has unused.yml + an unused composite action, exercising
    // the non-empty case for both arrays.
    let tmp = fresh_simple_fixture();
    let stdout = String::from_utf8(
        run(tmp.path(), &["orphans", "--format", "json"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    // shape: {"workflows": [...], "actions": [{"id": "...", "kind": "..."}, ...]}
    // — when empty: same shape with empty arrays. Mirrors `impact --format json`.
    let workflows = v
        .get("workflows")
        .and_then(Value::as_array)
        .expect("workflows array");
    let actions = v
        .get("actions")
        .and_then(Value::as_array)
        .expect("actions array");
    assert!(
        workflows
            .iter()
            .any(|w| w.as_str() == Some(".github/workflows/unused.yml")),
        "expected unused.yml in workflows: {workflows:?}"
    );
    let unused_entry = actions
        .iter()
        .find(|a| a.get("id").and_then(Value::as_str) == Some(".github/actions/unused"))
        .unwrap_or_else(|| panic!("expected unused composite in actions: {actions:?}"));
    assert_eq!(
        unused_entry.get("kind").and_then(Value::as_str),
        Some("composite"),
        "unused entry must carry kind: {unused_entry:?}",
    );
}

/// Locks the per-kind label format for `orphans` text + JSON output. The
/// `mixed-action-types` fixture exposes one unused composite, one unused JS
/// (node20), and one unused Docker action so all three `local-action-<kind>`
/// rows appear in a single invocation. Reverting the per-kind output in
/// `cli.rs` collapses the three rows back to a single `composite` label and
/// breaks the assertions below — exactly the regression this fixture is
/// designed to catch.
#[test]
fn orphans_mixed_action_types_emits_per_kind_labels() {
    let tmp = fresh_fixture("synthetic/mixed-action-types");

    // --- text output ---
    let text_stdout = String::from_utf8(
        run(tmp.path(), &["orphans"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        text_stdout.contains("composite   .github/actions/unused-composite"),
        "expected `local-action-composite` row for unused-composite: {text_stdout}",
    );
    assert!(
        text_stdout.contains("javascript  .github/actions/unused-js"),
        "expected `local-action-javascript` row for unused-js: {text_stdout}",
    );
    assert!(
        text_stdout.contains("docker      .github/actions/unused-docker"),
        "expected `local-action-docker` row for unused-docker: {text_stdout}",
    );

    // --- JSON output ---
    let json = run_orphans_json(tmp.path());
    let actions = json
        .get("actions")
        .and_then(Value::as_array)
        .expect("actions array");
    assert_eq!(
        actions.len(),
        3,
        "expected 3 unused local actions (composite, js, docker), got: {actions:?}"
    );
    let kinds: BTreeSet<&str> = actions
        .iter()
        .filter_map(|a| a.get("kind").and_then(Value::as_str))
        .collect();
    let expected: BTreeSet<&str> = ["composite", "javascript", "docker"].into_iter().collect();
    assert_eq!(
        kinds, expected,
        "actions[].kind must cover all three kinds exactly once: {actions:?}",
    );
    for entry in actions {
        let id = entry.get("id").and_then(Value::as_str).unwrap_or("");
        let kind = entry.get("kind").and_then(Value::as_str).unwrap_or("");
        let expected_id = match kind {
            "composite" => ".github/actions/unused-composite",
            "javascript" => ".github/actions/unused-js",
            "docker" => ".github/actions/unused-docker",
            _ => panic!("unexpected kind: {entry:?}"),
        };
        assert_eq!(
            id, expected_id,
            "kind/id pairing wrong: kind={kind} id={id}",
        );
    }
}

#[test]
fn wiring_json_output() {
    // Build a fixture with two distinct wiring kinds: UnannotatedDispatch from a
    // `gh workflow run` call without a matching `# ravelact:` annotation, and
    // DanglingAnnotation from an `# ravelact:dispatches` comment whose target
    // contains `..` (rejected by path validation).
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let wf_dir = root.join(".github/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();

    // Workflow A: unannotated `gh workflow run target.yml` → UnannotatedDispatch.
    std::fs::write(
        wf_dir.join("trigger_unannotated.yaml"),
        r#"
name: TriggerUnannotated
on: workflow_dispatch
jobs:
  fan:
    runs-on: ubuntu-latest
    steps:
      - run: gh workflow run target.yml
"#,
    )
    .unwrap();

    // Workflow B: dangling ravelact annotation pointing at `../bad` →
    // DanglingAnnotation (path-validation rejects `..` segments).
    std::fs::write(
        wf_dir.join("trigger_dangling.yaml"),
        r#"# ravelact:dispatches ../bad
name: TriggerDangling
on: workflow_dispatch
jobs:
  fan:
    runs-on: ubuntu-latest
    steps:
      - run: echo "dangling annotation lives at the workflow level"
"#,
    )
    .unwrap();

    run(root, &["build"]).success();

    // `wiring` exits 1 when findings are reported (Check-group contract).
    let stdout = String::from_utf8(
        run(root, &["wiring", "--format", "json"])
            .failure()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = v.as_array().expect("top-level array");
    assert!(arr.len() >= 2, "expected ≥2 findings: {stdout}");

    // Each finding has flattened (file, line) + tagged (kind + variant fields).
    for f in arr {
        assert!(f.get("file").is_some(), "finding lacks `file`: {f}");
        assert!(f.get("line").is_some(), "finding lacks `line`: {f}");
        assert!(f.get("kind").is_some(), "finding lacks `kind`: {f}");
    }

    let kinds: BTreeSet<String> = arr
        .iter()
        .filter_map(|e| e.get("kind").and_then(Value::as_str).map(String::from))
        .collect();
    assert!(
        kinds.contains("UnannotatedDispatch"),
        "expected UnannotatedDispatch in: {kinds:?}"
    );
    assert!(
        kinds.contains("DanglingAnnotation"),
        "expected DanglingAnnotation in: {kinds:?}"
    );
}

// -- stdin / `-` sentinel coverage (issue #75) --------------------------------

#[test]
fn impact_reads_stdin_when_no_args() {
    let tmp = fresh_simple_fixture();
    // Positional form: baseline output to compare against.
    let positional = String::from_utf8(
        run(
            tmp.path(),
            &["impact", ".github/workflows/build.yml", "--format", "json"],
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    // Stdin form: pipe the same path. Args are empty; stdin is read.
    let piped = String::from_utf8(
        run_capture_with_stdin(
            tmp.path(),
            &["impact", "--format", "json"],
            ".github/workflows/build.yml\n",
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    assert_eq!(
        piped, positional,
        "stdin form must match positional form. positional={positional}, piped={piped}"
    );
}

#[test]
fn impact_dash_sentinel_mixes_with_args() {
    let tmp = fresh_simple_fixture();
    // Pipe build.yml via stdin, supply setup as positional, with `-` mixed in.
    // Both inputs should reach query::impact::impact.
    let piped = String::from_utf8(
        run_capture_with_stdin(
            tmp.path(),
            &["impact", "-", ".github/actions/setup", "--format", "json"],
            ".github/workflows/build.yml\n",
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    let v: Value = serde_json::from_str(&piped).expect("valid JSON");
    let workflows = v
        .get("workflows")
        .and_then(Value::as_array)
        .expect("workflows array");
    let names: Vec<&str> = workflows.iter().filter_map(Value::as_str).collect();
    // setup is a composite consumed by build.yml & ci.yml — so ci.yml is in
    // the impact set even though we only piped build.yml + setup. ci.yml is
    // the unambiguous downstream caller of build.yml.
    assert!(
        names.contains(&".github/workflows/ci.yml"),
        "expected ci.yml from build.yml -> ci.yml chain (stdin + dash splice + positional setup): {piped}"
    );
}

#[test]
fn impact_rejects_nul_in_stdin_line() {
    let tmp = fresh_simple_fixture();
    let assert = run_capture_with_stdin(tmp.path(), &["impact"], "foo\0bar\n").failure();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("null byte"),
        "expected 'null byte' in stderr, got: {stderr}"
    );
}

#[test]
fn callers_reads_multiple_targets_from_stdin_json() {
    let tmp = fresh_simple_fixture();
    let stdout = String::from_utf8(
        run_capture_with_stdin(
            tmp.path(),
            &["callers", "--format", "json"],
            ".github/workflows/build.yml\n.github/actions/setup\n",
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    let v: Value = serde_json::from_str(&stdout).expect("valid JSON");
    let arr = v.as_array().expect("top-level array");
    assert_eq!(arr.len(), 2, "expected 2 entries, got: {stdout}");
    assert_eq!(
        arr[0].get("target").and_then(Value::as_str),
        Some(".github/workflows/build.yml"),
        "first target must echo input order: {stdout}"
    );
    assert_eq!(
        arr[1].get("target").and_then(Value::as_str),
        Some(".github/actions/setup"),
        "second target must echo input order: {stdout}"
    );
    // Each entry has a `hits` array (may be empty for some inputs).
    for e in arr {
        assert!(
            e.get("hits").and_then(Value::as_array).is_some(),
            "every entry must carry a hits array: {e}"
        );
    }
}

#[test]
fn callers_reads_multiple_targets_from_stdin_text() {
    let tmp = fresh_simple_fixture();
    let stdout = String::from_utf8(
        run_capture_with_stdin(
            tmp.path(),
            &["callers"],
            ".github/workflows/build.yml\n.github/actions/setup\n",
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    let h1 = stdout
        .find(".github/workflows/build.yml\n  1 caller\n")
        .unwrap_or_else(|| panic!("missing build.yml sub-section: {stdout}"));
    let h2 = stdout
        .find(".github/actions/setup\n  2 callers\n")
        .unwrap_or_else(|| panic!("missing setup sub-section: {stdout}"));
    assert!(
        h1 < h2,
        "headers must appear in input order. stdout: {stdout}"
    );
}

// ----- trace --branch / --tag / --path filters (issue #82) ---------------

/// Two minimal entry-points that differ only in their `branches:` filter.
/// Lets the `--branch` test assert the active workflow appears and the
/// inactive one does not.
fn write_two_branch_workflows(root: &Path) {
    let dir = root.join(".github/workflows");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main-only.yml"),
        r#"
name: Main Only
on:
  push:
    branches: [main]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo main
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("feat-only.yml"),
        r#"
name: Feat Only
on:
  push:
    branches: [feat-only]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo feat
"#,
    )
    .unwrap();
}

#[test]
fn trace_filters_by_branch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    write_two_branch_workflows(root);

    let stdout = String::from_utf8(
        run(root, &["trace", "push", "--branch", "main"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains(".github/workflows/main-only.yml"),
        "main-only.yml must appear for --branch main: {stdout}"
    );
    assert!(
        !stdout.contains(".github/workflows/feat-only.yml"),
        "feat-only.yml must NOT appear for --branch main: {stdout}"
    );
}

#[test]
fn trace_filters_by_tag() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let dir = root.join(".github/workflows");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("v-tag.yml"),
        r#"
name: V Tag
on:
  push:
    tags: ['v*']
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo v
"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("rel-tag.yml"),
        r#"
name: Release Tag
on:
  push:
    tags: ['release-*']
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo rel
"#,
    )
    .unwrap();

    let stdout = String::from_utf8(
        run(root, &["trace", "push", "--tag", "v1.0"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout.contains(".github/workflows/v-tag.yml"),
        "v-tag.yml must appear for --tag v1.0: {stdout}"
    );
    assert!(
        !stdout.contains(".github/workflows/rel-tag.yml"),
        "rel-tag.yml must NOT appear for --tag v1.0: {stdout}"
    );
}

#[test]
fn trace_filters_by_path_include() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let dir = root.join(".github/workflows");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("src-watcher.yml"),
        r#"
name: Src Watcher
on:
  push:
    paths: ['src/**']
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo src
"#,
    )
    .unwrap();

    let stdout_hit = String::from_utf8(
        run(root, &["trace", "push", "--path", "src/foo.rs"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout_hit.contains(".github/workflows/src-watcher.yml"),
        "src-watcher.yml must appear for --path src/foo.rs: {stdout_hit}"
    );

    let stdout_miss = String::from_utf8(
        run(root, &["trace", "push", "--path", "docs/x.md"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout_miss.contains("paths=[docs/x.md]"),
        "no-match header must mention `paths=[docs/x.md]`: {stdout_miss}"
    );
    assert!(
        !stdout_miss.contains(".github/workflows/src-watcher.yml"),
        "src-watcher.yml must NOT appear for --path docs/x.md: {stdout_miss}"
    );
}

#[test]
fn trace_filters_by_paths_ignore() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let dir = root.join(".github/workflows");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("not-docs.yml"),
        r#"
name: Not Docs
on:
  push:
    paths-ignore: ['docs/**']
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo not-docs
"#,
    )
    .unwrap();

    // Single-file changeset {docs/x.md} is fully ignored → no fire.
    let stdout_miss = String::from_utf8(
        run(root, &["trace", "push", "--path", "docs/x.md"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        !stdout_miss.contains(".github/workflows/not-docs.yml"),
        "not-docs.yml must NOT appear for paths-ignore single-file changeset of docs/x.md: {stdout_miss}"
    );

    // Single-file changeset {src/foo.rs} is not ignored → fires.
    let stdout_hit = String::from_utf8(
        run(root, &["trace", "push", "--path", "src/foo.rs"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout_hit.contains(".github/workflows/not-docs.yml"),
        "not-docs.yml must appear for --path src/foo.rs: {stdout_hit}"
    );
}

#[test]
fn trace_filters_combined_branch_and_path() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let dir = root.join(".github/workflows");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("main-src.yml"),
        r#"
name: Main Src
on:
  push:
    branches: [main]
    paths: ['src/**']
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo main-src
"#,
    )
    .unwrap();

    // Both filters satisfied → fires.
    let stdout_hit = String::from_utf8(
        run(
            root,
            &["trace", "push", "--branch", "main", "--path", "src/foo.rs"],
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    assert!(
        stdout_hit.contains(".github/workflows/main-src.yml"),
        "main-src.yml must appear when both filters satisfied: {stdout_hit}"
    );

    // Path mismatch → AND fails.
    let stdout_path_miss = String::from_utf8(
        run(
            root,
            &["trace", "push", "--branch", "main", "--path", "docs/x.md"],
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    assert!(
        !stdout_path_miss.contains(".github/workflows/main-src.yml"),
        "main-src.yml must NOT appear when path filter rejects: {stdout_path_miss}"
    );

    // Branch mismatch → AND fails.
    let stdout_branch_miss = String::from_utf8(
        run(
            root,
            &["trace", "push", "--branch", "feat", "--path", "src/foo.rs"],
        )
        .success()
        .get_output()
        .stdout
        .clone(),
    )
    .unwrap();
    assert!(
        !stdout_branch_miss.contains(".github/workflows/main-src.yml"),
        "main-src.yml must NOT appear when branch filter rejects: {stdout_branch_miss}"
    );
}

#[test]
fn trace_branch_filter_with_negation() {
    // GHA docs example: branches: [releases/**, !releases/**-alpha]
    // — releases/10 fires; releases/10-alpha does not.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let dir = root.join(".github/workflows");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("releases-non-alpha.yml"),
        r#"
name: Releases Non Alpha
on:
  push:
    branches: ['releases/**', '!releases/**-alpha']
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo non-alpha
"#,
    )
    .unwrap();

    let stdout_hit = String::from_utf8(
        run(root, &["trace", "push", "--branch", "releases/10"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        stdout_hit.contains(".github/workflows/releases-non-alpha.yml"),
        "releases/10 must hit (positive pattern matches, no negation suppress): {stdout_hit}"
    );

    let stdout_miss = String::from_utf8(
        run(root, &["trace", "push", "--branch", "releases/10-alpha"])
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap();
    assert!(
        !stdout_miss.contains(".github/workflows/releases-non-alpha.yml"),
        "releases/10-alpha must miss (negation pattern subtracts): {stdout_miss}"
    );
}
