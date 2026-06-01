//! Integration snapshots for the M2 external-finding overlay.
//!
//! Each test runs a real `ravelact` invocation against the committed
//! `zizmor-findings` fixture with `--findings <fixture>/zizmor.sarif` and
//! snapshots stdout. Non-regression (output unchanged without `--findings`) is
//! covered by the existing per-fixture `e2e__*@zizmor-findings` snapshots.

mod common;

use std::path::{Path, PathBuf};

use assert_cmd::Command;

use common::test_state_dir;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic/zizmor-findings")
}

fn sarif_arg() -> String {
    fixture()
        .join("zizmor.sarif")
        .to_string_lossy()
        .into_owned()
}

/// Path to the actionlint SARIF generated against the same zizmor estate. Used
/// to exercise multi-source overlay (`load_enriched`'s `Vec<PathBuf>` loop).
fn actionlint_sarif_arg() -> String {
    fixture()
        .join("actionlint.sarif")
        .to_string_lossy()
        .into_owned()
}

/// Run `ravelact --root <fixture> <args>` and capture stdout (expects success).
fn run(args: &[&str]) -> String {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("NO_COLOR", "1");
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    cmd.arg("--root").arg(fixture()).args(args);
    let assert = cmd.assert().success();
    String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout")
}

// ---- impact --------------------------------------------------------------

#[test]
fn impact_text_show_findings() {
    let out = run(&[
        "impact",
        ".github/workflows/ci.yml",
        "--findings",
        &sarif_arg(),
        "--show-findings",
    ]);
    insta::assert_snapshot!("impact_text_show_findings", out);
}

#[test]
fn impact_text_show_priority() {
    let out = run(&[
        "impact",
        ".github/workflows/ci.yml",
        "--findings",
        &sarif_arg(),
        "--show-findings",
        "--show-priority",
    ]);
    insta::assert_snapshot!("impact_text_show_priority", out);
}

#[test]
fn impact_markdown_show_findings() {
    let out = run(&[
        "impact",
        ".github/workflows/ci.yml",
        "--findings",
        &sarif_arg(),
        "--show-findings",
        "--format",
        "markdown",
    ]);
    insta::assert_snapshot!("impact_markdown_show_findings", out);
}

/// Multi-source overlay: two `--findings` files (zizmor + actionlint) are
/// concatenated by `load_enriched` and overlaid together. Both tools flag the
/// same untrusted-input steps (`ci.yml:14`, `pr-target.yml:15`); this test
/// targets `ci.yml`, so its snapshot shows the `ci.yml:14` overlap — both a
/// `zizmor`-sourced and an `actionlint`-sourced finding on one node. actionlint
/// rows show the bare `kind` (`expression`) while zizmor rows show the
/// source-stripped id (`template-injection`) — expected asymmetry.
#[test]
fn impact_text_multi_source() {
    let out = run(&[
        "impact",
        ".github/workflows/ci.yml",
        "--findings",
        &sarif_arg(),
        "--findings",
        &actionlint_sarif_arg(),
        "--show-findings",
    ]);
    insta::assert_snapshot!("impact_text_multi_source", out);
}

#[test]
fn impact_json_includes_findings() {
    let out = run(&[
        "impact",
        ".github/workflows/ci.yml",
        "--findings",
        &sarif_arg(),
        "--format",
        "json",
    ]);
    insta::assert_snapshot!("impact_json_includes_findings", out);
}

// ---- callers -------------------------------------------------------------

#[test]
fn callers_text_show_findings_orphan_target() {
    // orphan-tool has no callers; its own findings still surface as the
    // (degenerate) blast radius.
    let out = run(&[
        "callers",
        ".github/actions/orphan-tool",
        "--findings",
        &sarif_arg(),
        "--show-findings",
    ]);
    insta::assert_snapshot!("callers_text_show_findings_orphan_target", out);
}

#[test]
fn callers_text_show_priority() {
    let out = run(&[
        "callers",
        ".github/actions/orphan-tool",
        "--findings",
        &sarif_arg(),
        "--show-findings",
        "--show-priority",
    ]);
    insta::assert_snapshot!("callers_text_show_priority", out);
}

#[test]
fn callers_markdown_show_findings() {
    let out = run(&[
        "callers",
        ".github/workflows/ci.yml",
        "--findings",
        &sarif_arg(),
        "--show-findings",
        "--format",
        "markdown",
    ]);
    insta::assert_snapshot!("callers_markdown_show_findings", out);
}

#[test]
fn callers_json_includes_findings() {
    let out = run(&[
        "callers",
        ".github/workflows/ci.yml",
        "--findings",
        &sarif_arg(),
        "--format",
        "json",
    ]);
    insta::assert_snapshot!("callers_json_includes_findings", out);
}

// ---- orphans -------------------------------------------------------------

#[test]
fn orphans_text_show_findings() {
    // orphan-tool is an unused action carrying findings -> prefer deletion.
    let out = run(&["orphans", "--findings", &sarif_arg(), "--show-findings"]);
    insta::assert_snapshot!("orphans_text_show_findings", out);
}

#[test]
fn orphans_text_show_priority() {
    let out = run(&[
        "orphans",
        "--findings",
        &sarif_arg(),
        "--show-findings",
        "--show-priority",
    ]);
    insta::assert_snapshot!("orphans_text_show_priority", out);
}

#[test]
fn orphans_markdown_show_findings() {
    let out = run(&[
        "orphans",
        "--findings",
        &sarif_arg(),
        "--show-findings",
        "--format",
        "markdown",
    ]);
    insta::assert_snapshot!("orphans_markdown_show_findings", out);
}

#[test]
fn orphans_json_includes_findings() {
    let out = run(&["orphans", "--findings", &sarif_arg(), "--format", "json"]);
    insta::assert_snapshot!("orphans_json_includes_findings", out);
}

// ---- trace ---------------------------------------------------------------

#[test]
fn trace_tree_show_findings() {
    let out = run(&[
        "trace",
        "pull_request_target",
        "--ascii",
        "--findings",
        &sarif_arg(),
        "--show-findings",
    ]);
    insta::assert_snapshot!("trace_tree_show_findings", out);
}

#[test]
fn trace_tree_show_priority() {
    let out = run(&[
        "trace",
        "pull_request_target",
        "--ascii",
        "--findings",
        &sarif_arg(),
        "--show-findings",
        "--show-priority",
    ]);
    insta::assert_snapshot!("trace_tree_show_priority", out);
}

#[test]
fn trace_table_show_findings() {
    let out = run(&[
        "trace",
        "pull_request_target",
        "--format",
        "table",
        "--findings",
        &sarif_arg(),
        "--show-findings",
    ]);
    insta::assert_snapshot!("trace_table_show_findings", out);
}

#[test]
fn trace_markdown_show_findings() {
    let out = run(&[
        "trace",
        "pull_request_target",
        "--format",
        "markdown",
        "--findings",
        &sarif_arg(),
        "--show-findings",
    ]);
    insta::assert_snapshot!("trace_markdown_show_findings", out);
}

#[test]
fn trace_json_includes_findings() {
    let out = run(&[
        "trace",
        "pull_request_target",
        "--format",
        "json",
        "--findings",
        &sarif_arg(),
    ]);
    insta::assert_snapshot!("trace_json_includes_findings", out);
}

// ---- graph ---------------------------------------------------------------

#[test]
fn graph_highlight_findings_text() {
    let out = run(&[
        "graph",
        "--findings",
        &sarif_arg(),
        "--highlight",
        "findings",
    ]);
    insta::assert_snapshot!("graph_highlight_findings_text", out);
}

#[test]
fn graph_highlight_findings_show_priority() {
    let out = run(&[
        "graph",
        "--findings",
        &sarif_arg(),
        "--highlight",
        "findings",
        "--show-priority",
    ]);
    insta::assert_snapshot!("graph_highlight_findings_show_priority", out);
}

#[test]
fn graph_highlight_findings_markdown() {
    let out = run(&[
        "graph",
        "--findings",
        &sarif_arg(),
        "--highlight",
        "findings",
        "--format",
        "markdown",
    ]);
    insta::assert_snapshot!("graph_highlight_findings_markdown", out);
}

#[test]
fn graph_findings_without_highlight_is_unchanged() {
    // `--findings` without `--highlight findings` must not alter the Mermaid.
    let with_findings = run(&["graph", "--findings", &sarif_arg()]);
    let plain = run(&["graph"]);
    assert_eq!(with_findings, plain);
}
