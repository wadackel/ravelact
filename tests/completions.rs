//! Integration tests for shell-completion support.
//!
//! All tests invoke the built binary via `assert_cmd::Command::cargo_bin("ravelact")`
//! (no `cargo run` recompilation per test). Tests that mutate environment variables
//! (`COMPLETE`, `_CLAP_COMPLETE_INDEX`, `_CLAP_IFS`) are annotated `#[serial]` to
//! avoid races between parallel test threads.

use assert_cmd::Command;
use serial_test::serial;
use std::fs;
use tempfile::TempDir;

mod common;
use common::test_state_dir;

fn bin() -> Command {
    let mut c = Command::cargo_bin("ravelact").expect("binary should be built");
    c.env("XDG_STATE_HOME", test_state_dir());
    c.env("HOME", test_state_dir());
    c
}

fn run_completion_with_index(shell: &str, index: usize, words: &[&str]) -> String {
    let mut cmd = bin();
    cmd.env("COMPLETE", shell)
        .env("_CLAP_COMPLETE_INDEX", index.to_string())
        .env("_CLAP_IFS", "\n")
        .arg("--");
    for w in words {
        cmd.arg(w);
    }
    let out = cmd.output().expect("binary should execute");
    assert!(
        out.status.success(),
        "completion invocation failed: stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn test_completion_bash_outputs_setup() {
    let out = bin().args(["completion", "bash"]).output().unwrap();
    assert!(out.status.success(), "completion bash should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("source") && stdout.contains("COMPLETE=bash ravelact"),
        "expected source / COMPLETE=bash ravelact in: {stdout}"
    );
}

#[test]
fn test_completion_zsh_outputs_setup() {
    let out = bin().args(["completion", "zsh"]).output().unwrap();
    assert!(out.status.success(), "completion zsh should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("source") && stdout.contains("COMPLETE=zsh ravelact"),
        "expected source / COMPLETE=zsh ravelact in: {stdout}"
    );
}

#[test]
fn test_completion_fish_outputs_setup_without_gh_shim() {
    let out = bin().args(["completion", "fish"]).output().unwrap();
    assert!(out.status.success(), "completion fish should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("COMPLETE=fish ravelact"),
        "expected COMPLETE=fish ravelact in: {stdout}"
    );
    assert!(
        !stdout.contains("complete -c gh"),
        "fish completion must not install a gh-extension shim: {stdout}"
    );
}

#[test]
fn test_completion_invalid_shell_fails() {
    let out = bin().args(["completion", "invalid"]).output().unwrap();
    assert!(
        !out.status.success(),
        "completion invalid should fail; stdout={}, stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[serial]
fn test_toplevel_empty_word_excludes_flags() {
    let stdout = run_completion_with_index("bash", 1, &["ravelact", ""]);
    for expected in ["trace", "triggers", "callers", "impact"] {
        assert!(
            stdout.contains(expected),
            "expected {expected} in toplevel completion: {stdout:?}"
        );
    }
    for banned in ["--root", "--help", "--no-cache"] {
        assert!(
            !stdout.contains(banned),
            "{banned} must not appear at empty toplevel word: {stdout:?}"
        );
    }
}

#[test]
#[serial]
fn test_toplevel_dash_includes_flags() {
    let stdout = run_completion_with_index("bash", 1, &["ravelact", "-"]);
    assert!(
        stdout.contains("--root") || stdout.contains("--no-cache") || stdout.contains("--help"),
        "expected at least one flag (--root/--no-cache/--help) when current word is '-': {stdout:?}"
    );
}

#[test]
#[serial]
fn test_trace_event_completion() {
    let stdout = run_completion_with_index("bash", 2, &["ravelact", "trace", ""]);
    for expected in ["push", "pull_request", "workflow_dispatch"] {
        assert!(
            stdout.contains(expected),
            "expected event name {expected} in trace completion: {stdout:?}"
        );
    }
}

#[test]
#[serial]
fn test_callers_workflow_path_completion() {
    let tmp = TempDir::new().unwrap();
    let workflows_dir = tmp.path().join(".github/workflows");
    fs::create_dir_all(&workflows_dir).unwrap();
    fs::write(
        workflows_dir.join("foo.yaml"),
        "name: foo\non: push\njobs: {}\n",
    )
    .unwrap();

    let mut cmd = bin();
    let tmp_str = tmp.path().to_string_lossy().to_string();
    cmd.env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .env("_CLAP_IFS", "\n")
        .arg("--")
        .arg("ravelact")
        .arg("--root")
        .arg(&tmp_str)
        .arg("callers")
        .arg("");
    let out = cmd.output().expect("binary should execute");
    assert!(out.status.success(), "callers completion should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(".github/workflows/foo.yaml"),
        "expected .github/workflows/foo.yaml in callers completion candidates: {stdout:?}"
    );
}

/// `callers` completion must surface action **directories** (not `action.yml`
/// manifests) so the completed value resolves through `Target::from_user_input`
/// to the same `LocalAction.id` the IR stores. Round-trip: completion emits
/// `.github/actions/setup`, then `callers .github/actions/setup` lists the
/// workflow that uses it.
#[test]
#[serial]
fn test_callers_action_directory_completion() {
    let tmp = TempDir::new().unwrap();
    let workflows_dir = tmp.path().join(".github/workflows");
    let action_dir = tmp.path().join(".github/actions/setup");
    fs::create_dir_all(&workflows_dir).unwrap();
    fs::create_dir_all(&action_dir).unwrap();
    fs::write(
        workflows_dir.join("use-action.yaml"),
        r#"name: use-action
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: ./.github/actions/setup
"#,
    )
    .unwrap();
    fs::write(
        action_dir.join("action.yml"),
        r#"name: Setup
description: setup
runs:
  using: composite
  steps:
    - run: echo setup
      shell: bash
"#,
    )
    .unwrap();

    let tmp_str = tmp.path().to_string_lossy().to_string();

    // (a) completion shape — surface directory, hide manifest.
    let mut cmd = bin();
    cmd.env("COMPLETE", "bash")
        .env("_CLAP_COMPLETE_INDEX", "4")
        .env("_CLAP_IFS", "\n")
        .arg("--")
        .arg("ravelact")
        .arg("--root")
        .arg(&tmp_str)
        .arg("callers")
        .arg("");
    let out = cmd.output().expect("binary should execute");
    assert!(out.status.success(), "callers completion should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains(".github/actions/setup"),
        "expected action directory in completion: {stdout:?}"
    );
    assert!(
        !stdout.contains(".github/actions/setup/action.yml")
            && !stdout.contains(".github/actions/setup/action.yaml"),
        "completion must not leak the action.yml manifest path: {stdout:?}"
    );

    // (b) round-trip — the completed value must resolve to a real caller.
    let mut run = bin();
    run.arg("--root")
        .arg(&tmp_str)
        .arg("callers")
        .arg(".github/actions/setup");
    let run_out = run.output().expect("callers invocation");
    assert!(
        run_out.status.success(),
        "callers .github/actions/setup should succeed; stderr={}",
        String::from_utf8_lossy(&run_out.stderr)
    );
    let run_stdout = String::from_utf8_lossy(&run_out.stdout);
    assert!(
        run_stdout.contains(".github/workflows/use-action.yaml"),
        "callers must list the workflow that uses ./.github/actions/setup; got: {run_stdout:?}"
    );
}
