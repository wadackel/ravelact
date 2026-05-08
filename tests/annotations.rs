//! End-to-end tests for `# ravelact:` annotation support.
//! Run against `tests/fixtures/annotations/`.

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

mod common;
use common::test_state_dir;

fn fresh_annotations_fixture() -> TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/annotations");
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

fn stdout_for(root: &Path, args: &[&str]) -> String {
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    cmd.arg("--root").arg(root);
    for a in args {
        cmd.arg(a);
    }
    let assert = cmd.assert().success();
    String::from_utf8(assert.get_output().stdout.clone()).unwrap()
}

#[test]
fn trace_push_includes_annotated_dispatches() {
    let tmp = fresh_annotations_fixture();
    let out = stdout_for(tmp.path(), &["trace", "push"]);
    assert!(
        out.contains(".github/workflows/trigger_annotated.yml  [wf]"),
        "trigger_annotated must appear under push entry-point: {out}"
    );
    assert!(
        out.contains(".github/workflows/target.yml  [ann]  via dispatches"),
        "Annotated edge to target must render with `[ann]  via dispatches`: {out}"
    );
    // inside_run_block has ravelact inside a `run: |` block scalar — must NOT
    // show the annotated edge below it. With event-grouped output every entry
    // workflow is a depth-1 child of `╭─ push`, so the next sibling marker is
    // a column-0 `├─→` / `╰─→`. inside_run_block's own potential children
    // would be depth-2 (prefixed by `│   `). Walk the lines after the
    // inside_run_block.yml row and assert no `├─→` / `╰─→` appears before
    // hitting the next column-0 sibling.
    let irb_idx = out
        .find(".github/workflows/inside_run_block.yml")
        .expect("inside_run_block in trace");
    let after = &out[irb_idx..];
    let mut child_lines: Vec<&str> = Vec::new();
    for line in after.lines().skip(1) {
        if line.starts_with("├─→") || line.starts_with("╰─→") {
            break;
        }
        child_lines.push(line);
    }
    // EOF before another column-0 sibling is the same boundary: there are no
    // later lines where an annotated child could appear under this workflow.
    assert!(
        child_lines
            .iter()
            .all(|line| !line.contains("├─→") && !line.contains("╰─→")),
        "inside_run_block must have NO annotated children (block-scalar exclusion): {out}"
    );
}

#[test]
fn callers_target_lists_annotated_callers() {
    let tmp = fresh_annotations_fixture();
    let out = stdout_for(tmp.path(), &["callers", ".github/workflows/target.yml"]);
    // Step-anchored dispatches caller.
    assert!(
        out.contains(".github/workflows/trigger_annotated.yml")
            && out.contains("fan-out:0 via dispatches"),
        "expected step-anchored dispatches caller: {out}"
    );
    // Issue #83: Step.name appears as a suffix on the same line. The fixture's
    // step at index 0 has `name: kick off target` — verifies the
    // `Annotated::Step` arm of `format_caller_hit`.
    assert!(
        out.lines()
            .any(|l| l.contains(".github/workflows/trigger_annotated.yml")
                && l.contains("fan-out:0 via dispatches")
                && l.contains("name=\"kick off target\"")),
        "expected Annotated::Step name detail on the dispatches caller line: {out}"
    );
    // Workflow-level triggers caller (file-head comment).
    assert!(
        out.contains(".github/workflows/workflow_run_chain.yml")
            && out.contains("_workflow via triggers"),
        "expected workflow-anchored triggers caller: {out}"
    );
}

#[test]
fn impact_target_includes_annotation_only_callers() {
    let tmp = fresh_annotations_fixture();
    let out = stdout_for(tmp.path(), &["impact", ".github/workflows/target.yml"]);
    assert!(
        out.contains(".github/workflows/trigger_annotated.yml"),
        "annotation-only dispatcher must appear in impact: {out}"
    );
    assert!(
        out.contains(".github/workflows/workflow_run_chain.yml"),
        "workflow_run-chain caller (annotation-only) must appear in impact: {out}"
    );
}

#[test]
fn graph_includes_dotted_annotated_edges() {
    let tmp = fresh_annotations_fixture();
    let out = stdout_for(tmp.path(), &["graph"]);
    assert!(
        out.contains("-. dispatches .->"),
        "expected dotted dispatches edge in mermaid: {out}"
    );
    assert!(
        out.contains("-. triggers .->"),
        "expected dotted triggers edge in mermaid: {out}"
    );
    assert!(
        out.contains("classDef ravelactAnnotation"),
        "expected classDef directive when annotations present: {out}"
    );
}

// -- Task 9: wiring e2e ----------------------------------------------------

#[test]
fn wiring_detects_unannotated_dispatch_only() {
    let tmp = fresh_annotations_fixture();
    // `wiring` exits 1 when findings are reported (Check-group contract), so we
    // cannot use the success-asserting `stdout_for` helper here.
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    let assert = cmd
        .arg("--root")
        .arg(tmp.path())
        .arg("wiring")
        .assert()
        .failure();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    // trigger_unannotated must show up.
    assert!(
        out.contains("trigger_unannotated.yml"),
        "trigger_unannotated must be flagged: {out}"
    );
    assert!(
        out.contains("missing ravelact:dispatches annotation"),
        "expected the unannotated-dispatch message: {out}"
    );
    // trigger_annotated must NOT show up (annotation suppresses the finding).
    assert!(
        !out.contains("trigger_annotated.yml"),
        "trigger_annotated must NOT be flagged: {out}"
    );
    // inside_run_block must NOT show up (block-scalar exclusion + commented-out
    // shell line skipped).
    assert!(
        !out.contains("inside_run_block.yml"),
        "inside_run_block must NOT be flagged (block-scalar + comment): {out}"
    );
}

#[test]
fn wiring_exits_one_with_findings() {
    // Locks the Check-group contract: `wiring` exits 1 on findings.
    //
    // The annotations fixture is known to surface at least one finding
    // (the `gh workflow run target.yml` invocation in `trigger_unannotated.yml`
    // has no `# ravelact:dispatches` annotation). If the fixture is ever
    // changed to be clean, this assertion below will fail and the test must
    // be re-pointed to a fixture that produces findings.
    let tmp = fresh_annotations_fixture();
    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    let assert = cmd
        .arg("--root")
        .arg(tmp.path())
        .arg("wiring")
        .assert()
        .failure();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let exit = assert.get_output().status.code().expect("exit code");
    assert_eq!(
        exit, 1,
        "wiring must exit 1 when findings are reported (Check-group contract); got exit={exit}, stdout=\n{out}"
    );
    assert!(
        out.contains("missing ravelact:dispatches annotation"),
        "test premise: fixture must produce at least one finding so the exit-1 assertion is meaningful; got stdout=\n{out}"
    );
}

#[test]
fn wiring_exits_zero_when_no_findings() {
    // Locks the Check-group exit-0 path: a workflow set with no
    // `gh workflow run` invocations, no `on.workflow_run.workflows`, and
    // no `# ravelact:` comments produces zero findings, so `wiring` must
    // exit 0 and print the clean empty-state message.
    let tmp = tempfile::tempdir().expect("tempdir");
    let workflows = tmp.path().join(".github").join("workflows");
    std::fs::create_dir_all(&workflows).unwrap();
    std::fs::write(
        workflows.join("clean.yaml"),
        "name: clean\non: push\njobs:\n  noop:\n    runs-on: ubuntu-latest\n    steps:\n      - run: \":\"\n",
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("ravelact").unwrap();
    cmd.env("XDG_STATE_HOME", test_state_dir());
    cmd.env("HOME", test_state_dir());
    let assert = cmd
        .arg("--root")
        .arg(tmp.path())
        .arg("wiring")
        .assert()
        .success();
    let out = String::from_utf8(assert.get_output().stdout.clone()).unwrap();
    let exit = assert.get_output().status.code().expect("exit code");
    assert_eq!(
        exit, 0,
        "wiring must exit 0 when no findings are reported; got exit={exit}, stdout=\n{out}"
    );
    assert!(
        out.contains("wiring  no findings"),
        "expected clean empty-state message; got stdout=\n{out}"
    );
}
