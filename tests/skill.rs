//! Smoke tests for the `skill` subcommand against the built binary.
//!
//! Per-topic content checks live in the unit tests of `src/cli/skill.rs`,
//! which iterate every `Topic` variant; repeating the topic list here would
//! be a second copy to keep in sync.

use assert_cmd::Command;

mod common;
use common::test_state_dir;

fn bin() -> Command {
    let mut c = Command::cargo_bin("ravelact").expect("binary should be built");
    c.env("XDG_STATE_HOME", test_state_dir());
    c.env("HOME", test_state_dir());
    c
}

#[test]
fn skill_without_topic_prints_overview_and_topic_list() {
    let out = bin().arg("skill").output().unwrap();
    assert!(out.status.success(), "skill should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("# ravelact\n"),
        "overview heading: {stdout}"
    );
    for topic in ["callers", "findings", "recipes"] {
        assert!(
            stdout.contains(&format!("`{topic}`")),
            "topic list should name {topic}: {stdout}"
        );
    }
}

#[test]
fn skill_with_topic_prints_that_topic() {
    let out = bin().args(["skill", "impact"]).output().unwrap();
    assert!(out.status.success(), "skill impact should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("# `impact`\n"),
        "impact heading: {stdout}"
    );
}

#[test]
fn skill_with_unknown_topic_is_a_usage_error_listing_topics() {
    let out = bin().args(["skill", "nope"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "unknown topic exits 2");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("possible values") && stderr.contains("callers"),
        "stderr should list topics: {stderr}"
    );
}

#[test]
fn skill_runs_outside_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    let out = bin()
        .current_dir(dir.path())
        .args(["skill", "findings"])
        .output()
        .unwrap();
    assert!(out.status.success(), "skill should not need a repository");
    assert!(
        std::fs::read_dir(dir.path()).unwrap().next().is_none(),
        "skill must not write into the working directory"
    );
}

#[test]
fn help_lists_skill_subcommand() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success(), "--help should succeed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().any(|l| l.trim_start().starts_with("skill ")),
        "--help should list skill: {stdout}"
    );
}
