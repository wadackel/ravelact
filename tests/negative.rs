//! Malformed-input / error-path e2e suite (issue #147).
//!
//! The happy-path coverage in `tests/e2e.rs` and `tests/integration.rs` does
//! not assert how `ravelact` reacts to corrupted, cyclic, or otherwise
//! ill-formed inputs. Without these tests, a refactor that swaps `unwrap()`
//! for `?` (or vice versa) can silently degrade error behaviour on inputs the
//! suite never exercises.
//!
//! Each test asserts:
//!   1. the process exit code (most cases: `0` with the finding surfaced as
//!      output; some cases such as malformed YAML or wiring findings are
//!      legitimately non-zero and the test pins which one);
//!   2. a stable substring of the produced output (stderr or stdout, depending
//!      on which stream owns the diagnostic for that case) — never a full
//!      snapshot, so cosmetic message edits don't cascade into red CI;
//!   3. that the run does not panic (a panicking process fails the test by
//!      virtue of producing no assertion-matching output, and the panic is
//!      visible in the captured stderr).
//!
//! Inputs are constructed inline in `tempfile::TempDir` instances so that
//! cyclic / malformed fixtures are not committed to `tests/fixtures/` (which
//! is reserved for valid GHA estates).

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

mod common;
use common::{test_cache_path, test_state_dir};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Allocate a fresh tempdir for an inline negative fixture. Each test gets
/// its own root so parallel `cargo test` runs do not collide on cache writes.
fn empty_repo() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// Write `contents` to `path` relative to `root`, creating parent dirs.
fn write_file(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(&path, contents).expect("write file");
}

/// Build a `ravelact` invocation rooted at `root` with `args`, with color
/// disabled so substring assertions are not foiled by ANSI escapes. Cache
/// writes are redirected to a per-process tempdir via `XDG_STATE_HOME` so the
/// developer's real `~/.local/state/ravelact/` is never touched.
fn cmd(root: &Path, args: &[&str]) -> Command {
    let mut c = Command::cargo_bin("ravelact").expect("locate ravelact binary");
    c.env("NO_COLOR", "1");
    c.env("XDG_STATE_HOME", test_state_dir());
    c.env("HOME", test_state_dir());
    c.arg("--root").arg(root);
    for a in args {
        c.arg(a);
    }
    c
}

/// Path to the cache file written by `ravelact build` for this `root` under
/// the negative-test shared XDG state directory.
fn cache_path(root: &Path) -> PathBuf {
    test_cache_path(root)
}

/// A minimal, valid push-trigger workflow. Used by the cache-corruption tests
/// as a known-good baseline whose cache we can later mutate.
const TINY_VALID_WORKFLOW: &str = "\
name: tiny
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
";

// ---------------------------------------------------------------------------
// 1. Malformed YAML at workflow root
// ---------------------------------------------------------------------------

#[test]
fn malformed_yaml_unterminated_string_fails_with_parse_error() {
    let tmp = empty_repo();
    write_file(
        tmp.path(),
        ".github/workflows/bad.yaml",
        "name: bad\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: \"unterminated\n",
    );

    cmd(tmp.path(), &["dump"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("parse YAML"))
        .stderr(predicate::str::contains("bad.yaml"));
}

#[test]
fn malformed_yaml_tabs_in_indent_fails_with_parse_error() {
    let tmp = empty_repo();
    // YAML disallows tabs as indentation in block contexts. saphyr surfaces
    // this as "tabs disallowed within this context".
    write_file(
        tmp.path(),
        ".github/workflows/bad.yaml",
        "name: bad\non: push\njobs:\n\tbuild:\n\t  runs-on: ubuntu-latest\n\t  steps:\n\t    - run: echo hi\n",
    );

    cmd(tmp.path(), &["dump"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("tabs disallowed"));
}

#[test]
fn malformed_yaml_duplicate_top_level_keys_does_not_panic() {
    // saphyr accepts duplicate top-level keys (last wins). The contract this
    // test pins is "no panic, exit 0, output is parseable JSON" — i.e. the
    // tool still produces a usable IR rather than crashing on the malformed
    // input. If a future YAML library or duplicate-key check changes this to
    // a hard failure, update this test to match the new policy.
    let tmp = empty_repo();
    write_file(
        tmp.path(),
        ".github/workflows/dup.yaml",
        "name: dup\n\
         on: push\n\
         jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo a\n\
         jobs:\n  other:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo b\n",
    );

    let assert = cmd(tmp.path(), &["dump"]).assert().success().code(0);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    // The IR JSON must still be parseable — no panic, no truncation.
    let _: serde_json::Value = serde_json::from_str(&stdout).expect("dump emits valid JSON");
}

// ---------------------------------------------------------------------------
// 2. Dangling `uses:` shapes (typo'd dir, missing action.yml, file-instead-of-dir)
// ---------------------------------------------------------------------------

/// Workflow body that references `./.github/actions/<seg>` from a single
/// step. Used by the three dangling-uses sub-tests below.
fn workflow_with_local_uses(seg: &str) -> String {
    format!(
        "name: m\n\
         on: push\n\
         jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.github/actions/{seg}\n"
    )
}

#[test]
fn dangling_local_uses_typo_directory_surfaced_in_wiring() {
    let tmp = empty_repo();
    write_file(
        tmp.path(),
        ".github/workflows/m.yaml",
        &workflow_with_local_uses("typo"),
    );

    // `wiring` exits 1 when it reports any finding (per the CLI contract).
    cmd(tmp.path(), &["wiring"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("dangling-local-uses"))
        .stdout(predicate::str::contains("./.github/actions/typo"));
}

#[test]
fn dangling_local_uses_dir_without_action_yml_surfaced_in_wiring() {
    let tmp = empty_repo();
    // Directory exists but contains no action.yml / action.yaml.
    fs::create_dir_all(tmp.path().join(".github/actions/missing")).unwrap();
    write_file(
        tmp.path(),
        ".github/workflows/m.yaml",
        &workflow_with_local_uses("missing"),
    );

    cmd(tmp.path(), &["wiring"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("dangling-local-uses"))
        .stdout(predicate::str::contains("./.github/actions/missing"));
}

#[test]
fn dangling_local_uses_file_instead_of_directory_surfaced_in_wiring() {
    let tmp = empty_repo();
    // A regular file at the path the workflow expected to be an action dir.
    write_file(tmp.path(), ".github/actions/file", "not a directory\n");
    write_file(
        tmp.path(),
        ".github/workflows/m.yaml",
        &workflow_with_local_uses("file"),
    );

    cmd(tmp.path(), &["wiring"])
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::contains("dangling-local-uses"))
        .stdout(predicate::str::contains("./.github/actions/file"));
}

// ---------------------------------------------------------------------------
// 3. Cyclic `workflow_call` chain: A -> B -> A
// ---------------------------------------------------------------------------

#[test]
fn cyclic_workflow_call_chain_emits_cycle_marker_in_trace() {
    let tmp = empty_repo();
    write_file(
        tmp.path(),
        ".github/workflows/a.yaml",
        "name: A\non: workflow_call\njobs:\n  call_b:\n    uses: ./.github/workflows/b.yaml\n",
    );
    write_file(
        tmp.path(),
        ".github/workflows/b.yaml",
        "name: B\non: workflow_call\njobs:\n  call_a:\n    uses: ./.github/workflows/a.yaml\n",
    );
    // Entry workflow with `on: push` so `trace push` has a starting point.
    write_file(
        tmp.path(),
        ".github/workflows/entry.yaml",
        "name: Entry\non: push\njobs:\n  start:\n    uses: ./.github/workflows/a.yaml\n",
    );

    cmd(tmp.path(), &["trace", "push"])
        .assert()
        .success()
        .code(0)
        // The cycle-guard in `query::trace` emits a `[cyc]` leaf
        // for the second visit to A inside B's subtree.
        .stdout(predicate::str::contains("[cyc]"))
        .stdout(predicate::str::contains(".github/workflows/a.yaml"));
}

// ---------------------------------------------------------------------------
// 4. Cyclic local-action chain: composite X -> composite Y -> X
// ---------------------------------------------------------------------------

#[test]
fn cyclic_composite_action_chain_emits_cycle_marker_in_trace() {
    let tmp = empty_repo();
    write_file(
        tmp.path(),
        ".github/workflows/m.yaml",
        "name: m\non: push\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.github/actions/x\n",
    );
    write_file(
        tmp.path(),
        ".github/actions/x/action.yml",
        "name: X\nruns:\n  using: composite\n  steps:\n    - uses: ./.github/actions/y\n      shell: bash\n",
    );
    write_file(
        tmp.path(),
        ".github/actions/y/action.yml",
        "name: Y\nruns:\n  using: composite\n  steps:\n    - uses: ./.github/actions/x\n      shell: bash\n",
    );

    cmd(tmp.path(), &["trace", "push"])
        .assert()
        .success()
        .code(0)
        .stdout(predicate::str::contains("[cyc]"))
        .stdout(predicate::str::contains(".github/actions/x"));
}

// ---------------------------------------------------------------------------
// 5. `action.yml` with missing `runs:` block
// ---------------------------------------------------------------------------

#[test]
fn action_missing_runs_block_emits_warning_and_succeeds() {
    let tmp = empty_repo();
    write_file(tmp.path(), ".github/workflows/m.yaml", TINY_VALID_WORKFLOW);
    // No `runs:` key — the action parser rejects this with `missing \`runs\``,
    // and `rebuild_ir_from_inventory` swallows the per-action error as a
    // `warn:` line so the rest of the build can complete.
    write_file(
        tmp.path(),
        ".github/actions/x/action.yml",
        "name: x\ndescription: x\n",
    );

    cmd(tmp.path(), &["build"])
        .assert()
        .success()
        .code(0)
        .stderr(predicate::str::contains("warn: parse action"))
        .stderr(predicate::str::contains("missing `runs`"));
}

// ---------------------------------------------------------------------------
// 6. `action.yml` with unknown `runs.using` value
// ---------------------------------------------------------------------------

#[test]
fn action_unknown_runs_using_emits_warning_and_succeeds() {
    let tmp = empty_repo();
    write_file(tmp.path(), ".github/workflows/m.yaml", TINY_VALID_WORKFLOW);
    write_file(
        tmp.path(),
        ".github/actions/x/action.yml",
        "name: x\ndescription: x\nruns:\n  using: rust2025\n",
    );

    cmd(tmp.path(), &["build"])
        .assert()
        .success()
        .code(0)
        .stderr(predicate::str::contains("warn: parse action"))
        .stderr(predicate::str::contains("unknown runs.using"))
        .stderr(predicate::str::contains("rust2025"));
}

// ---------------------------------------------------------------------------
// 7. Cache file with mismatched `schema_version` -> rebuild silently
// ---------------------------------------------------------------------------

#[test]
fn cache_with_mismatched_schema_version_is_rebuilt_silently() {
    let tmp = empty_repo();
    write_file(tmp.path(), ".github/workflows/m.yaml", TINY_VALID_WORKFLOW);

    // Seed the cache with a known-good build.
    cmd(tmp.path(), &["build"]).assert().success();
    let cache = cache_path(tmp.path());
    assert!(cache.exists(), "build should write the cache");

    // Mutate only the top-level schema_version so the document is still
    // structurally valid JSON but no longer matches the IR shape this binary
    // can load.
    let raw = fs::read_to_string(&cache).expect("read cache");
    let mut doc: serde_json::Value = serde_json::from_str(&raw).expect("cache is JSON");
    doc["schema_version"] = serde_json::json!(99_999u32);
    fs::write(&cache, serde_json::to_string(&doc).unwrap()).expect("rewrite cache");

    // A subsequent `dump` must succeed (silent rebuild) and the on-disk cache
    // must be rewritten with the binary's current schema_version.
    cmd(tmp.path(), &["dump"]).assert().success().code(0);

    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&cache).expect("read cache after"))
            .expect("cache JSON after");
    assert_ne!(
        after["schema_version"].as_u64(),
        Some(99_999),
        "schema_version mismatch must trigger a rebuild, not a passthrough",
    );
}

// ---------------------------------------------------------------------------
// 8. Cache file with corrupted JSON -> rebuild silently
// ---------------------------------------------------------------------------

#[test]
fn cache_with_corrupted_json_is_rebuilt_silently() {
    let tmp = empty_repo();
    write_file(tmp.path(), ".github/workflows/m.yaml", TINY_VALID_WORKFLOW);

    cmd(tmp.path(), &["build"]).assert().success();
    let cache = cache_path(tmp.path());

    // Truncated JSON object. Anything that fails `serde_json::from_str` works.
    fs::write(&cache, "{\"this is not valid json").expect("write garbage cache");

    cmd(tmp.path(), &["dump"]).assert().success().code(0);

    let raw = fs::read_to_string(&cache).expect("read cache after");
    let _: serde_json::Value =
        serde_json::from_str(&raw).expect("rebuilt cache should be valid JSON");
}

// ---------------------------------------------------------------------------
// 9. Workflow file at unsupported path (subdirectory of `.github/workflows/`)
// ---------------------------------------------------------------------------

#[test]
fn workflow_in_workflows_subdirectory_is_ignored() {
    let tmp = empty_repo();
    // GitHub Actions only consumes `.github/workflows/*.{yml,yaml}` at the
    // top level. Files in subdirectories must not be parsed (and must not
    // trip a YAML error — they're invisible to the discovery layer).
    write_file(
        tmp.path(),
        ".github/workflows/sub/buried.yaml",
        TINY_VALID_WORKFLOW,
    );

    let assert = cmd(tmp.path(), &["dump"]).assert().success().code(0);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    let workflows = v["workflows"].as_array().expect("workflows array");
    assert!(
        workflows.is_empty(),
        "files under `.github/workflows/<sub>/` must not be discovered, got {workflows:?}",
    );
}

// ---------------------------------------------------------------------------
// 10. Invalid annotation comment syntax -> skip, not panic
// ---------------------------------------------------------------------------

#[test]
fn invalid_ravelact_annotation_comments_are_skipped_with_diagnostic() {
    let tmp = empty_repo();
    // Two malformed annotation forms: an unknown verb and a missing target.
    // Both must produce a diagnostic and then be dropped — no panic.
    write_file(
        tmp.path(),
        ".github/workflows/m.yaml",
        "name: m\non: push\n\
         # ravelact:bogusverb foo\n\
         # ravelact:dispatches\n\
         jobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
    );

    cmd(tmp.path(), &["build"])
        .assert()
        .success()
        .code(0)
        .stderr(predicate::str::contains("unrecognised ravelact comment"));
}

// ---------------------------------------------------------------------------
// 11. Empty repo (no workflows, no actions) -> exit 0 with empty IR
// ---------------------------------------------------------------------------

#[test]
fn empty_repo_dump_produces_empty_ir() {
    let tmp = empty_repo();

    let assert = cmd(tmp.path(), &["dump"]).assert().success().code(0);
    let stdout = String::from_utf8(assert.get_output().stdout.clone()).expect("utf8 stdout");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(
        v["workflows"].as_array().is_some_and(|a| a.is_empty()),
        "empty repo should yield empty workflows, got {:?}",
        v["workflows"],
    );
    assert!(
        v["actions"].as_array().is_some_and(|a| a.is_empty()),
        "empty repo should yield empty actions, got {:?}",
        v["actions"],
    );
    assert!(
        v["external_actions"]
            .as_array()
            .is_some_and(|a| a.is_empty()),
        "empty repo should yield empty external_actions, got {:?}",
        v["external_actions"],
    );
}
