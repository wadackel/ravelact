use super::uses::{parse_docker_ref, parse_uses};
use super::*;
use std::io::Write;
use std::path::Path;
use tempfile::tempdir;

fn write_workflow(dir: &Path, rel: &str, content: &str) -> std::path::PathBuf {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    let mut f = std::fs::File::create(&p).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    p
}

#[test]
fn parses_workflow_with_jobs_uses_and_callees() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/ci.yml",
        r#"
name: CI
on:
  push:
    branches: [main]
  pull_request:
    types: [opened]
jobs:
  test:
    runs-on: ubuntu-latest
    permissions:
      contents: read
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/setup
      - run: cargo test
  call-build:
    needs: test
    uses: ./.github/workflows/build.yml
    with:
      artifact: foo
    secrets: inherit
"#,
    );

    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no ravelact comments → no diagnostics");
    assert_eq!(wf.id.0, ".github/workflows/ci.yml");
    assert_eq!(wf.name.as_deref(), Some("CI"));
    assert_eq!(wf.jobs.len(), 2);
    assert!(wf.source.line.is_some(), "saphyr should populate line");

    let test_job = &wf.jobs[0];
    assert_eq!(test_job.id.0, "test");
    assert_eq!(test_job.steps.len(), 3);
    match test_job.steps[0].uses.as_ref().unwrap() {
        UsesRef::External {
            owner,
            repo,
            gitref,
            ..
        } => {
            assert_eq!(owner, "actions");
            assert_eq!(repo, "checkout");
            assert_eq!(gitref, "v4");
        }
        other => panic!("expected External, got {other:?}"),
    }
    match test_job.steps[1].uses.as_ref().unwrap() {
        UsesRef::LocalAction(ActionId(p)) => assert_eq!(p, ".github/actions/setup"),
        other => panic!("expected LocalAction, got {other:?}"),
    }
    assert_eq!(test_job.steps[2].run.as_deref(), Some("cargo test"));

    let call_job = &wf.jobs[1];
    assert_eq!(call_job.needs, vec!["test"]);
    let calls = call_job.calls_workflow.as_ref().unwrap();
    match &calls.workflow_ref {
        WorkflowRef::Local(WorkflowId(p)) => assert_eq!(p, ".github/workflows/build.yml"),
        other => panic!("expected Local, got {other:?}"),
    }
    assert_eq!(calls.with.get("artifact").map(|s| s.as_str()), Some("foo"));
    assert!(matches!(calls.secrets, SecretsPass::Inherit));
}

#[test]
fn parses_workflow_call_signature() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/build.yml",
        r#"
on:
  workflow_call:
    inputs:
      artifact:
        required: true
        description: artifact name
    outputs:
      url:
        description: published url
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty());
    assert!(wf.source.line.is_some(), "saphyr should populate line");
    let inputs = wf.inputs().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].name, "artifact");
    assert!(inputs[0].required);
}

#[test]
fn job_environment_parses_scalar_and_mapping() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/env.yml",
        r#"
on: push
jobs:
  scalar_form:
    runs-on: ubuntu-latest
    environment: prod
    steps:
      - run: echo a
  mapping_form:
    runs-on: ubuntu-latest
    environment:
      name: prod
      url: https://example.com
    steps:
      - run: echo b
  mapping_no_name:
    runs-on: ubuntu-latest
    environment:
      url: https://example.com
    steps:
      - run: echo c
  no_env:
    runs-on: ubuntu-latest
    steps:
      - run: echo d
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no ravelact comments => no diagnostics");
    let by_id: std::collections::HashMap<&str, &Job> =
        wf.jobs.iter().map(|j| (j.id.0.as_str(), j)).collect();
    assert_eq!(
        by_id["scalar_form"]
            .environment
            .as_ref()
            .map(|e| e.name.as_str()),
        Some("prod"),
        "scalar form should yield Some(\"prod\")"
    );
    assert_eq!(
        by_id["scalar_form"]
            .environment
            .as_ref()
            .and_then(|e| e.url.as_deref()),
        None,
        "scalar form should have no url"
    );
    assert_eq!(
        by_id["mapping_form"]
            .environment
            .as_ref()
            .map(|e| e.name.as_str()),
        Some("prod"),
        "mapping form with name: should yield Some(\"prod\")"
    );
    assert_eq!(
        by_id["mapping_form"]
            .environment
            .as_ref()
            .and_then(|e| e.url.as_deref()),
        Some("https://example.com"),
        "mapping form should capture url"
    );
    assert_eq!(
        by_id["mapping_no_name"].environment, None,
        "mapping without name: key should yield None"
    );
    assert_eq!(
        by_id["no_env"].environment, None,
        "missing environment: should yield None"
    );
}

#[test]
fn parses_uses_external_with_subpath() {
    let parsed = parse_uses("octo/awesome/sub/path@v2").unwrap();
    match parsed {
        UsesRef::External {
            owner,
            repo,
            subpath,
            gitref,
        } => {
            assert_eq!(owner, "octo");
            assert_eq!(repo, "awesome");
            assert_eq!(subpath.as_deref(), Some("sub/path"));
            assert_eq!(gitref, "v2");
        }
        other => panic!("expected External, got {other:?}"),
    }
}

#[test]
fn input_default_round_trips_all_scalar_kinds() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/default-roundtrip.yml",
        r#"
on:
  workflow_call:
    inputs:
      flag:
        default: true
      count:
        default: 42
      pi:
        default: 3.14
      name:
        default: "abc"
      nullable:
        default: ~
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no ravelact comments => no diagnostics");
    let inputs = wf.inputs().expect("inputs populated");
    let by_name: std::collections::HashMap<&str, &InputDecl> =
        inputs.iter().map(|i| (i.name.as_str(), i)).collect();

    assert_eq!(by_name["flag"].default.as_deref(), Some("true"));
    assert_eq!(by_name["count"].default.as_deref(), Some("42"));
    assert_eq!(by_name["pi"].default.as_deref(), Some("3.14"));
    assert_eq!(by_name["name"].default.as_deref(), Some("abc"));
    assert_eq!(by_name["nullable"].default, None);
}

#[test]
fn decls_drop_non_string_and_empty_keys() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/decl-drop.yml",
        r#"
on:
  workflow_call:
    inputs:
      valid_name:
        description: ok
      true:
        description: ng-bool
      "":
        description: ng-empty
    outputs:
      valid_name:
        description: ok
      true:
        description: ng-bool
      "":
        description: ng-empty
    secrets:
      valid_name:
        description: ok
      true:
        description: ng-bool
      "":
        description: ng-empty
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no ravelact comments => no diagnostics");

    let inputs = wf.inputs().expect("inputs populated");
    assert_eq!(inputs.len(), 1, "non-string and empty keys must drop");
    assert_eq!(inputs[0].name, "valid_name");

    let outputs = wf.outputs().expect("outputs populated");
    assert_eq!(outputs.len(), 1, "non-string and empty keys must drop");
    assert_eq!(outputs[0].name, "valid_name");

    let secrets = wf.secrets_required().expect("secrets_required populated");
    assert_eq!(secrets.len(), 1, "non-string and empty keys must drop");
    assert_eq!(secrets[0].name, "valid_name");
}

// ----- Test #3: parser preserves `types:` for non-PR events ----------
#[test]
fn parser_preserves_types_for_issues_event() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/issues.yml",
        r#"
on:
  issues:
    types: [labeled]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, _) = parse_workflow(&path, root).unwrap();
    assert_eq!(wf.triggers.len(), 1);
    assert_eq!(wf.triggers[0].event, EventKind::Issues);
    assert_eq!(wf.triggers[0].types, Some(vec!["labeled".into()]));
}

// ----- Test #4: parser preserves `types:` for Other events -----------
#[test]
fn parser_preserves_types_for_other_unmodeled_event() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/custom.yml",
        r#"
on:
  my_custom_event:
    types: [foo, bar]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, _) = parse_workflow(&path, root).unwrap();
    assert_eq!(wf.triggers.len(), 1);
    assert_eq!(
        wf.triggers[0].event,
        EventKind::Other {
            name: "my_custom_event".into()
        }
    );
    assert_eq!(wf.triggers[0].types, Some(vec!["foo".into(), "bar".into()]));
}

// ----- Test #5: branches-ignore → RefFilter::Exclude ------------------
#[test]
fn parser_branches_ignore_yields_exclude() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/exclude.yml",
        r#"
on:
  push:
    branches-ignore: [release/*]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, _) = parse_workflow(&path, root).unwrap();
    match &wf.triggers[0].branches {
        RefFilter::Exclude { patterns } => {
            assert_eq!(patterns, &vec!["release/*".to_string()]);
        }
        other => panic!("expected Exclude, got {other:?}"),
    }
}

// ----- Test #6: schedule per-entry timezone --------------------------
#[test]
fn parser_schedule_preserves_per_entry_timezone() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/sched.yml",
        r#"
on:
  schedule:
    - cron: '0 9 * * *'
      timezone: America/New_York
    - cron: '0 18 * * *'
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, _) = parse_workflow(&path, root).unwrap();
    let extras = wf.triggers[0]
        .extras
        .as_ref()
        .expect("schedule extras populated");
    let entries = match extras {
        EventExtras::Schedule { entries } => entries,
        other => panic!("expected Schedule extras, got {other:?}"),
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].cron, "0 9 * * *");
    assert_eq!(entries[0].timezone.as_deref(), Some("America/New_York"));
    assert_eq!(entries[1].cron, "0 18 * * *");
    assert_eq!(entries[1].timezone, None);
}

// ----- Test #8: 3 yaml shapes for `on:` -------------------------------
#[test]
fn parser_on_string_form() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/string.yml",
        r#"
on: push
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, _) = parse_workflow(&path, root).unwrap();
    assert_eq!(wf.triggers.len(), 1);
    assert_eq!(wf.triggers[0].event, EventKind::Push);
    assert_eq!(wf.triggers[0].branches, RefFilter::None);
    assert_eq!(wf.triggers[0].types, None);
    assert_eq!(wf.triggers[0].extras, None);
}

#[test]
fn parser_on_sequence_form() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/seq.yml",
        r#"
on: [push, pull_request]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, _) = parse_workflow(&path, root).unwrap();
    assert_eq!(wf.triggers.len(), 2);
    assert_eq!(wf.triggers[0].event, EventKind::Push);
    assert_eq!(wf.triggers[1].event, EventKind::PullRequest);
}

#[test]
fn parser_on_map_form_with_branches() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/map.yml",
        r#"
on:
  push:
    branches: [main]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, _) = parse_workflow(&path, root).unwrap();
    assert_eq!(wf.triggers[0].event, EventKind::Push);
    match &wf.triggers[0].branches {
        RefFilter::Include { patterns } => {
            assert_eq!(patterns, &vec!["main".to_string()]);
        }
        other => panic!("expected Include, got {other:?}"),
    }
}

#[test]
fn runs_on_scalar_form() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/ci.yml",
        r#"
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no diagnostics expected");
    let job = &wf.jobs[0];
    let runs_on = job.runs_on.as_ref().expect("runs_on must be set");
    assert_eq!(runs_on.labels, vec!["ubuntu-latest"]);
    assert!(runs_on.group.is_none());
}

#[test]
fn runs_on_sequence_form() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/ci.yml",
        r#"
on: push
jobs:
  test:
    runs-on: [self-hosted, linux, x64]
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no diagnostics expected");
    let job = &wf.jobs[0];
    let runs_on = job.runs_on.as_ref().expect("runs_on must be set");
    assert_eq!(
        runs_on.labels,
        vec![
            "self-hosted".to_string(),
            "linux".to_string(),
            "x64".to_string()
        ]
    );
    assert!(runs_on.group.is_none());
}

#[test]
fn runs_on_mapping_form() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/ci.yml",
        r#"
on: push
jobs:
  test:
    runs-on:
      group: my-runners
      labels: [linux]
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no diagnostics expected");
    let job = &wf.jobs[0];
    let runs_on = job.runs_on.as_ref().expect("runs_on must be set");
    assert_eq!(runs_on.labels, vec!["linux"]);
    assert_eq!(runs_on.group.as_deref(), Some("my-runners"));
}

#[test]
fn runs_on_missing_emits_diagnostic_for_non_uses_job() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/ci.yml",
        r#"
on: push
jobs:
  no-runner:
    steps:
      - run: echo hi
"#,
    );
    let (_wf, diags) = parse_workflow(&path, root).unwrap();
    assert_eq!(diags.len(), 1, "expected exactly one diagnostic");
    assert!(
        diags[0].message.contains("no-runner"),
        "diagnostic should mention the job id: {:?}",
        diags[0].message
    );
    assert!(
        diags[0].message.contains("runs-on"),
        "diagnostic should mention runs-on: {:?}",
        diags[0].message
    );
}

#[test]
fn runs_on_absent_on_uses_job_is_not_a_diagnostic() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/ci.yml",
        r#"
on: push
jobs:
  call-build:
    uses: ./.github/workflows/build.yml
"#,
    );
    let (_wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(
        diags.is_empty(),
        "uses-job without runs-on must not emit a diagnostic, got: {diags:?}"
    );
}

#[test]
fn repository_dispatch_types_never_emit_diagnostic() {
    // repository_dispatch accepts user-defined types — open set, no validation.
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/repo-dispatch.yml",
        r#"
on:
  repository_dispatch:
    types: [my-custom-event, another-custom-event]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (_, diags) = parse_workflow(&path, root).unwrap();
    assert!(
        diags.is_empty(),
        "repository_dispatch types are user-defined and must never trigger diagnostics"
    );
}

#[test]
fn merge_group_unknown_type_emits_diagnostic() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/mg-typo.yml",
        r#"
on:
  merge_group:
    types: [checks_requested, unknown_type]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (_, diags) = parse_workflow(&path, root).unwrap();
    assert_eq!(diags.len(), 1);
    assert!(diags[0].message.contains("unknown_type"));
    assert!(diags[0].message.contains("merge_group"));
}

#[test]
fn unknown_event_other_types_never_emit_diagnostic() {
    // Other { name } has no closed set — forward-compat, no validation.
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/other-event.yml",
        r#"
on:
  some_future_event:
    types: [whatever]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (_, diags) = parse_workflow(&path, root).unwrap();
    assert!(
        diags.is_empty(),
        "unknown events (Other variant) must not trigger type diagnostics"
    );
}

#[test]
fn watch_valid_type_no_diagnostic() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/watch.yml",
        r#"
on:
  watch:
    types: [started]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (_, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "watch:started is valid: {diags:?}");
}

#[test]
fn job_strategy_parses_matrix_scalars_and_fail_fast() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/matrix.yml",
        r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      max-parallel: 3
      matrix:
        os: [ubuntu-latest, windows-latest]
        node: [18, 20]
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty());
    let job = &wf.jobs[0];
    let strategy = job.strategy.as_ref().expect("strategy captured");
    assert_eq!(strategy.fail_fast, Some(false));
    assert_eq!(strategy.max_parallel, Some(3));
    let matrix = strategy.matrix.as_ref().expect("matrix captured");
    let os = matrix.dimensions.get("os").expect("os dimension");
    assert_eq!(
        os,
        &vec![
            MatrixValue::String("ubuntu-latest".into()),
            MatrixValue::String("windows-latest".into()),
        ]
    );
    let node = matrix.dimensions.get("node").expect("node dimension");
    assert_eq!(node, &vec![MatrixValue::Int(18), MatrixValue::Int(20)]);
}

#[test]
fn job_strategy_parses_object_matrix_values() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/matrix-obj.yml",
        r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        config:
          - { os: ubuntu-latest, arch: x86_64 }
          - { os: windows-latest, arch: aarch64 }
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty());
    let job = &wf.jobs[0];
    let matrix = job
        .strategy
        .as_ref()
        .expect("strategy")
        .matrix
        .as_ref()
        .expect("matrix");
    let config = matrix.dimensions.get("config").expect("config dimension");
    assert_eq!(config.len(), 2);
    match &config[0] {
        MatrixValue::Object(map) => {
            assert_eq!(
                map.get("os"),
                Some(&MatrixValue::String("ubuntu-latest".into()))
            );
            assert_eq!(map.get("arch"), Some(&MatrixValue::String("x86_64".into())));
        }
        other => panic!("expected Object, got {other:?}"),
    }
}

#[test]
fn job_strategy_absent_yields_none() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/no-strategy.yml",
        r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, _) = parse_workflow(&path, root).unwrap();
    assert!(wf.jobs[0].strategy.is_none());
}

#[test]
fn parse_docker_ref_hub_image_with_tag() {
    // docker://alpine:3.8 — no host, has tag
    let d = parse_docker_ref("alpine:3.8");
    assert_eq!(d.host, None);
    assert_eq!(d.image, "alpine");
    assert_eq!(d.tag.as_deref(), Some("3.8"));
}

#[test]
fn parse_docker_ref_registry_with_path_and_tag() {
    // docker://gcr.io/cloud-builders/gradle — host present, path image, no tag
    let d = parse_docker_ref("gcr.io/cloud-builders/gradle");
    assert_eq!(d.host.as_deref(), Some("gcr.io"));
    assert_eq!(d.image, "cloud-builders/gradle");
    assert_eq!(d.tag, None);
}

#[test]
fn parse_docker_ref_registry_with_tag() {
    // docker://ghcr.io/owner/image:latest — host present, path image, has tag
    let d = parse_docker_ref("ghcr.io/owner/image:latest");
    assert_eq!(d.host.as_deref(), Some("ghcr.io"));
    assert_eq!(d.image, "owner/image");
    assert_eq!(d.tag.as_deref(), Some("latest"));
}

#[test]
fn parse_docker_ref_hub_image_no_tag() {
    // docker://ubuntu — no host, no tag
    let d = parse_docker_ref("ubuntu");
    assert_eq!(d.host, None);
    assert_eq!(d.image, "ubuntu");
    assert_eq!(d.tag, None);
}

#[test]
fn parse_uses_docker_roundtrip() {
    let uses = parse_uses("docker://alpine:3.8").unwrap();
    match uses {
        UsesRef::Docker(d) => {
            assert_eq!(d.host, None);
            assert_eq!(d.image, "alpine");
            assert_eq!(d.tag.as_deref(), Some("3.8"));
        }
        other => panic!("expected Docker, got {other:?}"),
    }
}

// ----- defaults and env (workflow-level and job-level) -------------------
#[test]
fn parses_workflow_and_job_defaults_and_env() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/defaults-env.yml",
        r#"
on: push
defaults:
  run:
    shell: bash
    working-directory: src
env:
  WORKFLOW_VAR: hello
  PORT: "8080"
jobs:
  build:
    runs-on: ubuntu-latest
    defaults:
      run:
        shell: sh
        working-directory: scripts
    env:
      JOB_VAR: world
    steps:
      - run: echo hi
  no-defaults:
    runs-on: ubuntu-latest
    steps:
      - run: echo bye
"#,
    );

    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no diagnostics expected");

    // Workflow-level defaults
    let wf_defaults = wf.defaults.as_ref().expect("workflow defaults present");
    let wf_run = wf_defaults
        .run
        .as_ref()
        .expect("workflow defaults.run present");
    assert_eq!(wf_run.shell.as_deref(), Some("bash"));
    assert_eq!(wf_run.working_directory.as_deref(), Some("src"));

    // Workflow-level env
    assert_eq!(
        wf.env.get("WORKFLOW_VAR").map(|s| s.as_str()),
        Some("hello")
    );
    assert_eq!(wf.env.get("PORT").map(|s| s.as_str()), Some("8080"));
    assert_eq!(wf.env.len(), 2);

    let by_id: std::collections::HashMap<&str, &Job> =
        wf.jobs.iter().map(|j| (j.id.0.as_str(), j)).collect();

    // Job-level defaults
    let build = by_id["build"];
    let job_defaults = build.defaults.as_ref().expect("job defaults present");
    let job_run = job_defaults.run.as_ref().expect("job defaults.run present");
    assert_eq!(job_run.shell.as_deref(), Some("sh"));
    assert_eq!(job_run.working_directory.as_deref(), Some("scripts"));

    // Job-level env
    assert_eq!(build.env.get("JOB_VAR").map(|s| s.as_str()), Some("world"));

    // Job with no defaults or env
    let no_defaults = by_id["no-defaults"];
    assert!(
        no_defaults.defaults.is_none(),
        "no-defaults job has no defaults"
    );
    assert!(no_defaults.env.is_empty(), "no-defaults job has no env");
}

// ----- Concurrency parsing --------------------------------------------

#[test]
fn parser_workflow_concurrency_scalar_form() {
    // `concurrency: my-group` collapses to { group: "my-group", cancel_in_progress: None }
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/conc-scalar.yml",
        r#"
on: push
concurrency: my-group
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty());
    let conc = wf.concurrency.as_ref().expect("concurrency should be Some");
    assert_eq!(conc.group, "my-group");
    assert_eq!(conc.cancel_in_progress, None);
}

#[test]
fn parser_workflow_concurrency_map_form_with_cancel() {
    // Map form with cancel-in-progress: true
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/conc-map.yml",
        r#"
on: push
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty());
    let conc = wf.concurrency.as_ref().expect("concurrency should be Some");
    assert_eq!(conc.group, "${{ github.workflow }}-${{ github.ref }}");
    assert_eq!(conc.cancel_in_progress, Some(true));
}

#[test]
fn parser_workflow_concurrency_map_form_explicit_false() {
    // cancel-in-progress: false must be stored as Some(false), not None
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/conc-map-false.yml",
        r#"
on: push
concurrency:
  group: deploy
  cancel-in-progress: false
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty());
    let conc = wf.concurrency.as_ref().expect("concurrency should be Some");
    assert_eq!(conc.group, "deploy");
    assert_eq!(conc.cancel_in_progress, Some(false));
}

#[test]
fn parser_workflow_no_concurrency_yields_none() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/no-conc.yml",
        r#"
on: push
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty());
    assert!(wf.concurrency.is_none());
}

#[test]
fn parser_job_concurrency_map_form() {
    // Job-level concurrency with cancel-in-progress
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/job-conc.yml",
        r#"
on: push
jobs:
  deploy:
    runs-on: ubuntu-latest
    concurrency:
      group: deploy-${{ github.ref }}
      cancel-in-progress: true
    steps:
      - run: echo deploy
  no_concurrency:
    runs-on: ubuntu-latest
    steps:
      - run: echo noop
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty());
    let by_id: std::collections::HashMap<&str, &Job> =
        wf.jobs.iter().map(|j| (j.id.0.as_str(), j)).collect();
    let deploy_conc = by_id["deploy"]
        .concurrency
        .as_ref()
        .expect("deploy job should have concurrency");
    assert_eq!(deploy_conc.group, "deploy-${{ github.ref }}");
    assert_eq!(deploy_conc.cancel_in_progress, Some(true));
    assert!(
        by_id["no_concurrency"].concurrency.is_none(),
        "job without concurrency: should yield None"
    );
}

#[test]
fn job_container_parses_scalar_and_mapping_forms() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/containers.yml",
        r#"
on: push
jobs:
  scalar_form:
    runs-on: ubuntu-latest
    container: alpine:3.20
    steps:
      - run: echo hi
  mapping_form:
    runs-on: ubuntu-latest
    container:
      image: node:20
      credentials:
        username: myuser
        password: ${{ secrets.REGISTRY_PASSWORD }}
      env:
        NODE_ENV: test
      ports:
        - "8080:80"
      volumes:
        - my_vol:/data
      options: --cpus 1
    steps:
      - run: echo hi
  no_container:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no ravelact comments => no diagnostics");
    let by_id: std::collections::HashMap<&str, &Job> =
        wf.jobs.iter().map(|j| (j.id.0.as_str(), j)).collect();

    // Scalar form
    let scalar = by_id["scalar_form"]
        .container
        .as_ref()
        .expect("scalar container");
    assert_eq!(scalar.image, "alpine:3.20");
    assert!(scalar.credentials.is_none());
    assert!(scalar.env.is_empty());
    assert!(scalar.ports.is_empty());
    assert!(scalar.volumes.is_empty());
    assert!(scalar.options.is_none());

    // Mapping form
    let mapping = by_id["mapping_form"]
        .container
        .as_ref()
        .expect("mapping container");
    assert_eq!(mapping.image, "node:20");
    let creds = mapping.credentials.as_ref().expect("credentials populated");
    assert_eq!(creds.username, "myuser");
    assert_eq!(creds.password, "${{ secrets.REGISTRY_PASSWORD }}");
    assert_eq!(
        mapping.env.get("NODE_ENV").map(|s| s.as_str()),
        Some("test")
    );
    assert_eq!(mapping.ports, vec!["8080:80"]);
    assert_eq!(mapping.volumes, vec!["my_vol:/data"]);
    assert_eq!(mapping.options.as_deref(), Some("--cpus 1"));

    // No container
    assert!(by_id["no_container"].container.is_none());
}

#[test]
fn job_services_parses_mapping_form() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/services.yml",
        r#"
on: push
jobs:
  with_services:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:15
        env:
          POSTGRES_PASSWORD: secret
        ports:
          - "5432:5432"
      redis:
        image: redis:7
    steps:
      - run: echo hi
  no_services:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
    );
    let (wf, diags) = parse_workflow(&path, root).unwrap();
    assert!(diags.is_empty(), "no ravelact comments => no diagnostics");
    let by_id: std::collections::HashMap<&str, &Job> =
        wf.jobs.iter().map(|j| (j.id.0.as_str(), j)).collect();

    let svcs = &by_id["with_services"].services;
    assert_eq!(svcs.len(), 2);

    let pg = svcs.get("postgres").expect("postgres service");
    assert_eq!(pg.image, "postgres:15");
    assert_eq!(
        pg.env.get("POSTGRES_PASSWORD").map(|s| s.as_str()),
        Some("secret")
    );
    assert_eq!(pg.ports, vec!["5432:5432"]);

    let redis = svcs.get("redis").expect("redis service");
    assert_eq!(redis.image, "redis:7");
    assert!(redis.env.is_empty());

    assert!(by_id["no_services"].services.is_empty());
}

// ----- Malformed `uses:` diagnostics ----------------------------------

/// A step `uses:` value that fails to parse (e.g. missing `@ref`) should
/// surface a `ParseDiagnostic` pinpointing the offending file and line
/// rather than silently dropping the dependency edge. Regression test for
/// issue #112.
#[test]
fn step_uses_missing_ref_emits_diagnostic() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let path = write_workflow(
        root,
        ".github/workflows/bad-uses.yml",
        // Line numbers (1-based):
        //   1: (blank, opening newline of the raw string)
        //   2: name: bad
        //   3: on: push
        //   4: jobs:
        //   5:   build:
        //   6:     runs-on: ubuntu-latest
        //   7:     steps:
        //   8:       - uses: actions/checkout    <-- diagnostic anchor
        r#"
name: bad
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout
"#,
    );

    let (wf, diags) = parse_workflow(&path, root).unwrap();

    assert_eq!(
        diags.len(),
        1,
        "expected exactly one ParseDiagnostic, got: {diags:?}"
    );
    let d = &diags[0];
    assert_eq!(d.file, path, "diagnostic file should match the workflow");
    assert_eq!(
        d.line, 8,
        "diagnostic should pin the line of the `uses:` value"
    );
    assert!(
        d.message.contains("uses"),
        "message should mention `uses`: {}",
        d.message
    );

    // The Step itself is preserved (so step indices stay stable) but its
    // `uses` field is `None` because the value failed to parse.
    let step = &wf.jobs[0].steps[0];
    assert!(step.uses.is_none(), "malformed uses should be None");
    assert_eq!(wf.jobs[0].steps.len(), 1, "step is still in the IR");
}

// ============================================================================
// Direct unit tests for pub(crate) / pub(super) parsing helpers.
//
// These tests load a YAML fragment via `saphyr::MarkedYaml::load_from_str` and
// pass the resulting node straight to a helper, bypassing `parse_workflow`.
// This lets us exercise edge cases (empty / null / malformed inputs) that are
// awkward to reach through a full workflow document.
// ============================================================================

use saphyr::{LoadableYamlNode, MarkedYaml};

/// Parse a YAML fragment and return the top-level node, ready to be handed to
/// a parser helper. Tests intentionally panic on parse failure.
fn yaml_node(src: &str) -> MarkedYaml<'static> {
    // Leak the source so the resulting `MarkedYaml<'static>` can outlive the
    // helper call without lifetime gymnastics inside each test.
    let leaked: &'static str = Box::leak(src.to_string().into_boxed_str());
    let mut docs = MarkedYaml::load_from_str(leaked).expect("parse YAML fragment");
    docs.pop().expect("at least one document")
}

// ----- parse_input_decls -----------------------------------------------------

mod parse_input_decls_tests {
    use super::*;
    use crate::parser::workflow::triggers::parse_input_decls;

    #[test]
    fn parse_input_decls_happy_path_collects_each_field() {
        let node = yaml_node(
            r#"
artifact:
  required: true
  default: "build.tar"
  description: artifact name
  type: string
flag:
  required: false
  default: false
  type: boolean
"#,
        );
        let decls = parse_input_decls(Some(&node));
        assert_eq!(decls.len(), 2);

        let by_name: std::collections::HashMap<&str, &InputDecl> =
            decls.iter().map(|d| (d.name.as_str(), d)).collect();
        let artifact = by_name["artifact"];
        assert!(artifact.required);
        assert_eq!(artifact.default.as_deref(), Some("build.tar"));
        assert!(matches!(artifact.input_type, Some(InputType::String)));

        let flag = by_name["flag"];
        assert!(!flag.required);
        assert_eq!(flag.default.as_deref(), Some("false"));
        assert!(matches!(flag.input_type, Some(InputType::Boolean)));
    }

    #[test]
    fn parse_input_decls_edge_none_value_returns_empty() {
        let decls = parse_input_decls(None);
        assert!(decls.is_empty());
    }

    #[test]
    fn parse_input_decls_edge_empty_mapping_returns_empty() {
        // `inputs: {}` — explicitly empty mapping.
        let node = yaml_node("{}");
        let decls = parse_input_decls(Some(&node));
        assert!(decls.is_empty());
    }

    #[test]
    fn parse_input_decls_malformed_non_mapping_returns_empty() {
        // YAML scalar — not a mapping, helper must reject silently.
        let node = yaml_node("not-a-mapping");
        let decls = parse_input_decls(Some(&node));
        assert!(decls.is_empty());
    }

    #[test]
    fn parse_input_decls_choice_type_collects_options() {
        let node = yaml_node(
            r#"
mode:
  type: choice
  options:
    - dev
    - prod
"#,
        );
        let decls = parse_input_decls(Some(&node));
        assert_eq!(decls.len(), 1);
        match &decls[0].input_type {
            Some(InputType::Choice { options }) => {
                assert_eq!(options, &vec!["dev".to_string(), "prod".to_string()]);
            }
            other => panic!("expected Choice, got {other:?}"),
        }
    }
}

// ----- parse_output_decls ----------------------------------------------------

mod parse_output_decls_tests {
    use super::*;
    use crate::parser::workflow::triggers::parse_output_decls;

    #[test]
    fn parse_output_decls_happy_path_captures_value_expression() {
        let node = yaml_node(
            r#"
url:
  description: deploy URL
  value: ${{ jobs.deploy.outputs.url }}
sha:
  value: ${{ steps.build.outputs.sha }}
"#,
        );
        let decls = parse_output_decls(Some(&node));
        assert_eq!(decls.len(), 2);
        let by_name: std::collections::HashMap<&str, &OutputDecl> =
            decls.iter().map(|d| (d.name.as_str(), d)).collect();
        assert_eq!(
            by_name["url"].value.as_deref(),
            Some("${{ jobs.deploy.outputs.url }}")
        );
        assert_eq!(
            by_name["sha"].value.as_deref(),
            Some("${{ steps.build.outputs.sha }}")
        );
    }

    #[test]
    fn parse_output_decls_edge_missing_value_field_yields_none() {
        let node = yaml_node(
            r#"
artifact:
  description: produced artifact
"#,
        );
        let decls = parse_output_decls(Some(&node));
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].name, "artifact");
        assert!(decls[0].value.is_none());
    }

    #[test]
    fn parse_output_decls_edge_none_returns_empty() {
        assert!(parse_output_decls(None).is_empty());
    }

    #[test]
    fn parse_output_decls_malformed_non_mapping_returns_empty() {
        let node = yaml_node("[a, b, c]");
        assert!(parse_output_decls(Some(&node)).is_empty());
    }
}

// ----- parse_secret_decls ----------------------------------------------------

mod parse_secret_decls_tests {
    use super::*;
    use crate::parser::workflow::triggers::parse_secret_decls;

    #[test]
    fn parse_secret_decls_happy_path_captures_required_flag() {
        let node = yaml_node(
            r#"
TOKEN:
  required: true
  description: deploy token
OPTIONAL_KEY:
  required: false
"#,
        );
        let decls = parse_secret_decls(Some(&node));
        assert_eq!(decls.len(), 2);
        let by_name: std::collections::HashMap<&str, &SecretDecl> =
            decls.iter().map(|d| (d.name.as_str(), d)).collect();
        assert!(by_name["TOKEN"].required);
        assert!(!by_name["OPTIONAL_KEY"].required);
    }

    #[test]
    fn parse_secret_decls_edge_missing_required_defaults_to_false() {
        let node = yaml_node(
            r#"
SECRET:
  description: just description, no required key
"#,
        );
        let decls = parse_secret_decls(Some(&node));
        assert_eq!(decls.len(), 1);
        assert!(!decls[0].required, "missing `required:` defaults to false");
    }

    #[test]
    fn parse_secret_decls_edge_none_returns_empty() {
        assert!(parse_secret_decls(None).is_empty());
    }

    #[test]
    fn parse_secret_decls_malformed_non_mapping_returns_empty() {
        let node = yaml_node("\"a string, not a map\"");
        assert!(parse_secret_decls(Some(&node)).is_empty());
    }
}

// ----- parse_default_scalar --------------------------------------------------

mod parse_default_scalar_tests {
    use super::*;
    use crate::parser::workflow::triggers::parse_default_scalar;

    #[test]
    fn parse_default_scalar_happy_string() {
        let node = yaml_node("\"hello\"");
        assert_eq!(parse_default_scalar(Some(&node)).as_deref(), Some("hello"));
    }

    #[test]
    fn parse_default_scalar_happy_int_and_bool_stringify() {
        let int_node = yaml_node("42");
        assert_eq!(parse_default_scalar(Some(&int_node)).as_deref(), Some("42"));
        let bool_node = yaml_node("true");
        assert_eq!(
            parse_default_scalar(Some(&bool_node)).as_deref(),
            Some("true")
        );
    }

    #[test]
    fn parse_default_scalar_edge_explicit_null_returns_none() {
        let node = yaml_node("~");
        assert_eq!(parse_default_scalar(Some(&node)), None);
    }

    #[test]
    fn parse_default_scalar_edge_missing_returns_none() {
        assert_eq!(parse_default_scalar(None), None);
    }

    #[test]
    fn parse_default_scalar_malformed_collection_yields_empty_string() {
        // Sequence falls through `stringify_value` to `String::new()`.
        // The value is technically malformed for `default:`, but the helper
        // must not panic — it returns Some("") so callers can decide what to
        // do with it.
        let node = yaml_node("[a, b]");
        let v = parse_default_scalar(Some(&node));
        assert_eq!(v.as_deref(), Some(""));
    }
}

// ----- parse_permissions -----------------------------------------------------

mod parse_permissions_tests {
    use super::*;
    use crate::parser::workflow::helpers::parse_permissions;

    #[test]
    fn parse_permissions_happy_read_all() {
        let node = yaml_node("read-all");
        let mut diags = Vec::new();
        let perms = parse_permissions(Some(&node), &mut diags, Path::new("test.yaml"));
        assert!(diags.is_empty());
        assert_eq!(perms, Some(Permissions::Coarse(CoarseKind::ReadAll)));
    }

    #[test]
    fn parse_permissions_happy_write_all() {
        let node = yaml_node("write-all");
        let mut diags = Vec::new();
        let perms = parse_permissions(Some(&node), &mut diags, Path::new("test.yaml"));
        assert!(diags.is_empty());
        assert_eq!(perms, Some(Permissions::Coarse(CoarseKind::WriteAll)));
    }

    #[test]
    fn parse_permissions_happy_explicit_scope_map() {
        let node = yaml_node(
            r#"
contents: read
pull-requests: write
id-token: none
"#,
        );
        let mut diags = Vec::new();
        let perms = parse_permissions(Some(&node), &mut diags, Path::new("test.yaml"));
        assert!(diags.is_empty(), "well-formed scopes => no diagnostics");
        match perms {
            Some(Permissions::Scopes(scopes)) => {
                assert_eq!(scopes.get(&ScopeKey::Contents), Some(&ScopeAccess::Read));
                assert_eq!(
                    scopes.get(&ScopeKey::PullRequests),
                    Some(&ScopeAccess::Write)
                );
                assert_eq!(scopes.get(&ScopeKey::IdToken), Some(&ScopeAccess::None));
            }
            other => panic!("expected Permissions::Scopes, got {other:?}"),
        }
    }

    #[test]
    fn parse_permissions_edge_empty_mapping_yields_empty_scopes() {
        // `permissions: {}` is the canonical "deny everything" form per spec.
        let node = yaml_node("{}");
        let mut diags = Vec::new();
        let perms = parse_permissions(Some(&node), &mut diags, Path::new("test.yaml"));
        assert!(diags.is_empty());
        match perms {
            Some(Permissions::Scopes(scopes)) => assert!(scopes.is_empty()),
            other => panic!("expected empty Scopes, got {other:?}"),
        }
    }

    #[test]
    fn parse_permissions_edge_missing_returns_none() {
        let mut diags = Vec::new();
        let perms = parse_permissions(None, &mut diags, Path::new("test.yaml"));
        assert!(perms.is_none());
        assert!(diags.is_empty());
    }

    #[test]
    fn parse_permissions_malformed_unknown_coarse_emits_diagnostic() {
        let node = yaml_node("everything");
        let mut diags = Vec::new();
        let perms = parse_permissions(Some(&node), &mut diags, Path::new("test.yaml"));
        assert_eq!(diags.len(), 1, "unknown coarse value emits a diagnostic");
        assert!(diags[0].message.contains("everything"));
        assert_eq!(
            perms,
            Some(Permissions::Coarse(CoarseKind::Unknown(
                "everything".into()
            )))
        );
    }

    #[test]
    fn parse_permissions_malformed_unknown_scope_key_emits_diagnostic() {
        let node = yaml_node(
            r#"
contents: read
not-a-real-scope: read
"#,
        );
        let mut diags = Vec::new();
        let perms = parse_permissions(Some(&node), &mut diags, Path::new("test.yaml"));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("not-a-real-scope"));
        match perms {
            Some(Permissions::Scopes(scopes)) => {
                // Both keys are kept — the unknown key gets ScopeKey::Unknown.
                assert_eq!(scopes.len(), 2);
                let has_unknown = scopes
                    .keys()
                    .any(|k| matches!(k, ScopeKey::Unknown(s) if s == "not-a-real-scope"));
                assert!(has_unknown, "scopes must include ScopeKey::Unknown entry");
            }
            other => panic!("expected Scopes, got {other:?}"),
        }
    }

    #[test]
    fn parse_permissions_malformed_unknown_access_value_emits_diagnostic() {
        let node = yaml_node("contents: maybe");
        let mut diags = Vec::new();
        let perms = parse_permissions(Some(&node), &mut diags, Path::new("test.yaml"));
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("maybe"));
        match perms {
            Some(Permissions::Scopes(scopes)) => {
                assert_eq!(
                    scopes.get(&ScopeKey::Contents),
                    Some(&ScopeAccess::Unknown("maybe".into()))
                );
            }
            other => panic!("expected Scopes, got {other:?}"),
        }
    }
}

// ----- parse_triggers (parse_on) — additional event coverage -----------------
//
// The pre-existing tests already cover string / sequence / map / branches /
// branches-ignore / schedule / activity-type validation. The cases below add
// the remaining listed shapes: pull_request, workflow_dispatch, workflow_run,
// workflow_call, mixed events, and a malformed `on:` document.

mod parse_triggers_tests {
    use super::*;

    fn parse_workflow_with_on(content: &str) -> Workflow {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = write_workflow(root, ".github/workflows/wf.yml", content);
        let (wf, diags) = parse_workflow(&path, root).unwrap();
        // Diagnostics are checked individually per test as needed.
        let _ = diags;
        wf
    }

    #[test]
    fn parse_triggers_pull_request_with_types_and_branches() {
        let wf = parse_workflow_with_on(
            r#"
on:
  pull_request:
    types: [opened, synchronize]
    branches: [main]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
        );
        assert_eq!(wf.triggers.len(), 1);
        let t = &wf.triggers[0];
        assert_eq!(t.event, EventKind::PullRequest);
        assert_eq!(
            t.types,
            Some(vec!["opened".to_string(), "synchronize".to_string()])
        );
        match &t.branches {
            RefFilter::Include { patterns } => {
                assert_eq!(patterns, &vec!["main".to_string()]);
            }
            other => panic!("expected Include, got {other:?}"),
        }
    }

    #[test]
    fn parse_triggers_workflow_dispatch_collects_inputs() {
        let wf = parse_workflow_with_on(
            r#"
on:
  workflow_dispatch:
    inputs:
      env:
        type: choice
        options: [dev, prod]
        default: dev
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
        );
        let extras = wf.triggers[0]
            .extras
            .as_ref()
            .expect("workflow_dispatch extras should be Some");
        match extras {
            EventExtras::WorkflowDispatch { inputs } => {
                assert_eq!(inputs.len(), 1);
                assert_eq!(inputs[0].name, "env");
                assert!(matches!(
                    inputs[0].input_type,
                    Some(InputType::Choice { .. })
                ));
                assert_eq!(inputs[0].default.as_deref(), Some("dev"));
            }
            other => panic!("expected WorkflowDispatch, got {other:?}"),
        }
    }

    #[test]
    fn parse_triggers_workflow_run_collects_workflows_list() {
        let wf = parse_workflow_with_on(
            r#"
on:
  workflow_run:
    workflows: [Build, Test]
    types: [completed]
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
        );
        let extras = wf.triggers[0]
            .extras
            .as_ref()
            .expect("workflow_run extras should be Some");
        match extras {
            EventExtras::WorkflowRun { workflows } => {
                assert_eq!(workflows, &vec!["Build".to_string(), "Test".to_string()]);
            }
            other => panic!("expected WorkflowRun, got {other:?}"),
        }
    }

    #[test]
    fn parse_triggers_workflow_call_collects_inputs_outputs_secrets() {
        let wf = parse_workflow_with_on(
            r#"
on:
  workflow_call:
    inputs:
      ref:
        required: true
    outputs:
      url:
        value: ${{ jobs.deploy.outputs.url }}
    secrets:
      TOKEN:
        required: true
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
        );
        match wf.triggers[0]
            .extras
            .as_ref()
            .expect("workflow_call extras should be Some")
        {
            EventExtras::WorkflowCall {
                inputs,
                outputs,
                secrets,
            } => {
                assert_eq!(inputs.len(), 1);
                assert!(inputs[0].required);
                assert_eq!(outputs.len(), 1);
                assert_eq!(
                    outputs[0].value.as_deref(),
                    Some("${{ jobs.deploy.outputs.url }}")
                );
                assert_eq!(secrets.len(), 1);
                assert!(secrets[0].required);
            }
            other => panic!("expected WorkflowCall, got {other:?}"),
        }
    }

    #[test]
    fn parse_triggers_mixed_events_preserve_each_entry() {
        let wf = parse_workflow_with_on(
            r#"
on:
  push:
    branches: [main]
  pull_request:
    types: [opened]
  schedule:
    - cron: "0 0 * * *"
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
        );
        assert_eq!(wf.triggers.len(), 3);
        let kinds: Vec<&EventKind> = wf.triggers.iter().map(|t| &t.event).collect();
        assert!(kinds.contains(&&EventKind::Push));
        assert!(kinds.contains(&&EventKind::PullRequest));
        assert!(kinds.contains(&&EventKind::Schedule));
    }

    #[test]
    fn parse_triggers_malformed_non_mapping_yields_empty() {
        // A nonsense `on:` value (integer) must produce zero triggers without
        // panicking. Other top-level fields still parse normally.
        let wf = parse_workflow_with_on(
            r#"
on: 42
jobs:
  noop:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
        );
        assert!(
            wf.triggers.is_empty(),
            "malformed `on:` should produce no triggers"
        );
    }
}

// ----- parse_job (matrix shapes, needs, if, permissions, outputs, secrets) ---

mod parse_job_tests {
    use super::*;

    fn parse_jobs_from(content: &str) -> Workflow {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = write_workflow(root, ".github/workflows/wf.yml", content);
        let (wf, _) = parse_workflow(&path, root).unwrap();
        wf
    }

    #[test]
    fn parse_job_happy_with_needs_if_outputs() {
        let wf = parse_jobs_from(
            r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    outputs:
      sha: ${{ steps.s1.outputs.sha }}
    steps:
      - id: s1
        run: echo "sha=abc" >> $GITHUB_OUTPUT
  deploy:
    runs-on: ubuntu-latest
    needs: [build]
    if: ${{ success() && github.ref == 'refs/heads/main' }}
    steps:
      - run: echo deploy
"#,
        );
        let by_id: std::collections::HashMap<&str, &Job> =
            wf.jobs.iter().map(|j| (j.id.0.as_str(), j)).collect();
        let build = by_id["build"];
        assert_eq!(
            build.outputs.get("sha").map(|s| s.as_str()),
            Some("${{ steps.s1.outputs.sha }}")
        );
        let deploy = by_id["deploy"];
        assert_eq!(deploy.needs, vec!["build".to_string()]);
        assert_eq!(
            deploy.if_expr.as_deref(),
            Some("${{ success() && github.ref == 'refs/heads/main' }}")
        );
    }

    #[test]
    fn parse_job_needs_accepts_scalar_string_form() {
        // `needs: prepare` (scalar) must be normalised to a single-element vec.
        let wf = parse_jobs_from(
            r#"
on: push
jobs:
  prepare:
    runs-on: ubuntu-latest
    steps:
      - run: echo prepare
  build:
    runs-on: ubuntu-latest
    needs: prepare
    steps:
      - run: echo build
"#,
        );
        let build = wf.jobs.iter().find(|j| j.id.0 == "build").unwrap();
        assert_eq!(build.needs, vec!["prepare".to_string()]);
    }

    #[test]
    fn parse_job_scoped_permissions_and_secrets_inherit() {
        let wf = parse_jobs_from(
            r#"
on: push
jobs:
  call:
    permissions:
      contents: read
      id-token: write
    uses: ./.github/workflows/build.yml
    secrets: inherit
"#,
        );
        let job = &wf.jobs[0];
        match job.permissions.as_ref().expect("scoped permissions") {
            Permissions::Scopes(scopes) => {
                assert_eq!(scopes.get(&ScopeKey::Contents), Some(&ScopeAccess::Read));
                assert_eq!(scopes.get(&ScopeKey::IdToken), Some(&ScopeAccess::Write));
            }
            other => panic!("expected job-scoped Permissions::Scopes, got {other:?}"),
        }
        let calls = job.calls_workflow.as_ref().expect("calls_workflow");
        assert!(matches!(calls.secrets, SecretsPass::Inherit));
    }

    #[test]
    fn parse_job_secrets_explicit_map_captures_pairs() {
        let wf = parse_jobs_from(
            r#"
on: push
jobs:
  call:
    uses: ./.github/workflows/build.yml
    secrets:
      TOKEN: ${{ secrets.GITHUB_TOKEN }}
      NPM_KEY: ${{ secrets.NPM_KEY }}
"#,
        );
        let calls = wf.jobs[0].calls_workflow.as_ref().expect("calls_workflow");
        match &calls.secrets {
            SecretsPass::Explicit(map) => {
                assert_eq!(
                    map.get("TOKEN").map(|s| s.as_str()),
                    Some("${{ secrets.GITHUB_TOKEN }}")
                );
                assert_eq!(
                    map.get("NPM_KEY").map(|s| s.as_str()),
                    Some("${{ secrets.NPM_KEY }}")
                );
            }
            other => panic!("expected Explicit, got {other:?}"),
        }
    }

    #[test]
    fn parse_job_edge_no_optional_fields() {
        let wf = parse_jobs_from(
            r#"
on: push
jobs:
  bare:
    runs-on: ubuntu-latest
    steps:
      - run: echo hi
"#,
        );
        let job = &wf.jobs[0];
        assert!(job.needs.is_empty());
        assert!(job.if_expr.is_none());
        assert!(job.permissions.is_none());
        assert!(job.outputs.is_empty());
        assert!(job.calls_workflow.is_none());
        assert!(job.strategy.is_none());
        assert!(job.environment.is_none());
        assert!(job.concurrency.is_none());
    }

    #[test]
    fn parse_job_malformed_jobs_value_yields_empty() {
        // `jobs:` as a sequence (not a mapping) is invalid YAML for the
        // workflow spec. parse_jobs returns an empty vec rather than panicking.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = write_workflow(
            root,
            ".github/workflows/wf.yml",
            r#"
on: push
jobs:
  - name: oops
"#,
        );
        let (wf, _diags) = parse_workflow(&path, root).unwrap();
        assert!(
            wf.jobs.is_empty(),
            "non-mapping `jobs:` must produce no jobs"
        );
    }
}

// ----- parse_step (uses / run / both, with, env, id) -------------------------

mod parse_step_tests {
    use super::*;

    fn first_job_steps(content: &str) -> Vec<Step> {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = write_workflow(root, ".github/workflows/wf.yml", content);
        let (wf, _) = parse_workflow(&path, root).unwrap();
        wf.jobs.into_iter().next().unwrap().steps
    }

    #[test]
    fn parse_step_uses_only_captures_uses_and_with() {
        let steps = first_job_steps(
            r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - id: checkout
        uses: actions/checkout@v4
        with:
          fetch-depth: 0
          path: src
"#,
        );
        assert_eq!(steps.len(), 1);
        let s = &steps[0];
        assert_eq!(s.id.as_ref().map(|i| i.0.as_str()), Some("checkout"));
        assert!(matches!(s.uses, Some(UsesRef::External { .. })));
        assert!(s.run.is_none());
        assert_eq!(
            s.with.get("fetch-depth").map(|v| v.as_str()),
            Some("0"),
            "with values are stringified"
        );
        assert_eq!(s.with.get("path").map(|v| v.as_str()), Some("src"));
    }

    #[test]
    fn parse_step_run_only_captures_run_and_env_scope() {
        let steps = first_job_steps(
            r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - name: build
        run: cargo build --release
        env:
          RUSTFLAGS: "-D warnings"
          CARGO_TERM_COLOR: always
"#,
        );
        assert_eq!(steps.len(), 1);
        let s = &steps[0];
        assert_eq!(s.name.as_deref(), Some("build"));
        assert_eq!(s.run.as_deref(), Some("cargo build --release"));
        assert!(s.uses.is_none());
        assert_eq!(
            s.env.get("RUSTFLAGS").map(|v| v.as_str()),
            Some("-D warnings")
        );
        assert_eq!(
            s.env.get("CARGO_TERM_COLOR").map(|v| v.as_str()),
            Some("always")
        );
    }

    #[test]
    fn parse_step_uses_and_run_both_present_keep_both_fields() {
        // GA disallows both `uses:` and `run:` on the same step, but the IR
        // captures whatever the YAML says — diagnostics live elsewhere.
        let steps = first_job_steps(
            r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        run: echo also-run
"#,
        );
        assert_eq!(steps.len(), 1);
        assert!(steps[0].uses.is_some(), "uses preserved");
        assert_eq!(
            steps[0].run.as_deref(),
            Some("echo also-run"),
            "run preserved"
        );
    }

    #[test]
    fn parse_step_edge_empty_step_mapping_yields_no_fields() {
        // An empty step ({}) — every optional field must default to None/empty.
        let steps = first_job_steps(
            r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - {}
"#,
        );
        assert_eq!(steps.len(), 1);
        let s = &steps[0];
        assert!(s.id.is_none());
        assert!(s.name.is_none());
        assert!(s.uses.is_none());
        assert!(s.run.is_none());
        assert!(s.with.is_empty());
        assert!(s.env.is_empty());
        assert_eq!(s.index, 0);
    }

    #[test]
    fn parse_step_indexes_are_zero_based_and_contiguous() {
        let steps = first_job_steps(
            r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - run: echo a
      - run: echo b
      - run: echo c
"#,
        );
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0].index, 0);
        assert_eq!(steps[1].index, 1);
        assert_eq!(steps[2].index, 2);
    }

    #[test]
    fn parse_step_malformed_steps_non_sequence_yields_no_steps() {
        // `steps:` as a mapping (instead of a sequence) is invalid; helper
        // returns an empty vec rather than panicking.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = write_workflow(
            root,
            ".github/workflows/wf.yml",
            r#"
on: push
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      not_a_list: true
"#,
        );
        let (wf, _diags) = parse_workflow(&path, root).unwrap();
        assert!(wf.jobs[0].steps.is_empty());
    }
}

// ----- strategy.matrix include / exclude -------------------------------------

mod parse_matrix_include_exclude_tests {
    use super::*;

    fn parse_matrix_for(content: &str) -> Matrix {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let path = write_workflow(root, ".github/workflows/wf.yml", content);
        let (wf, _) = parse_workflow(&path, root).unwrap();
        wf.jobs[0]
            .strategy
            .as_ref()
            .expect("strategy")
            .matrix
            .as_ref()
            .expect("matrix")
            .clone()
    }

    #[test]
    fn parse_matrix_include_clause_is_captured_as_dimension() {
        let matrix = parse_matrix_for(
            r#"
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
        include:
          - os: ubuntu-latest
            extra: dbg
    steps:
      - run: echo hi
"#,
        );
        let include = matrix
            .dimensions
            .get("include")
            .expect("include captured as dimension");
        assert_eq!(include.len(), 1);
        match &include[0] {
            MatrixValue::Object(map) => {
                assert_eq!(
                    map.get("os"),
                    Some(&MatrixValue::String("ubuntu-latest".into()))
                );
                assert_eq!(map.get("extra"), Some(&MatrixValue::String("dbg".into())));
            }
            other => panic!("expected Object inside include, got {other:?}"),
        }
    }

    #[test]
    fn parse_matrix_exclude_clause_is_captured_as_dimension() {
        let matrix = parse_matrix_for(
            r#"
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest]
        node: [18, 20]
        exclude:
          - os: windows-latest
            node: 18
    steps:
      - run: echo hi
"#,
        );
        let exclude = matrix
            .dimensions
            .get("exclude")
            .expect("exclude captured as dimension");
        assert_eq!(exclude.len(), 1);
        match &exclude[0] {
            MatrixValue::Object(map) => {
                assert_eq!(
                    map.get("os"),
                    Some(&MatrixValue::String("windows-latest".into()))
                );
                assert_eq!(map.get("node"), Some(&MatrixValue::Int(18)));
            }
            other => panic!("expected Object inside exclude, got {other:?}"),
        }
    }

    #[test]
    fn parse_matrix_include_and_exclude_coexist() {
        let matrix = parse_matrix_for(
            r#"
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        os: [ubuntu-latest, macos-latest]
        include:
          - os: macos-latest
            xcode: "15"
        exclude:
          - os: ubuntu-latest
    steps:
      - run: echo hi
"#,
        );
        assert!(matrix.dimensions.contains_key("os"));
        assert!(matrix.dimensions.contains_key("include"));
        assert!(matrix.dimensions.contains_key("exclude"));
        assert_eq!(matrix.dimensions["include"].len(), 1);
        assert_eq!(matrix.dimensions["exclude"].len(), 1);
    }

    #[test]
    fn parse_matrix_edge_non_sequence_dimension_is_skipped() {
        // A scalar value under a matrix key isn't a valid dimension; the
        // helper silently skips it, keeping the surrounding matrix usable.
        let matrix = parse_matrix_for(
            r#"
on: push
jobs:
  test:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        os: [ubuntu-latest]
        bogus: not-a-list
    steps:
      - run: echo hi
"#,
        );
        assert!(matrix.dimensions.contains_key("os"));
        assert!(
            !matrix.dimensions.contains_key("bogus"),
            "non-sequence dimension is dropped"
        );
    }
}
