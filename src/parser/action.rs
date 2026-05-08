use crate::ir::*;
use crate::parser::annotations::{
    attach_local_action_annotations, collect_block_scalar_ranges, scan_ravelact_comments,
};
use crate::parser::workflow::{
    as_str, get_field, parse_input_decls, parse_output_decls, parse_steps, path_to_forward,
};
use anyhow::{Context, Result};
use saphyr::{LoadableYamlNode, MarkedYaml};
use std::path::Path;

/// Parse an `action.yml` / `action.yaml` file. The action's [`ActionId`] is the
/// path of its directory, relative to `root`, with forward slashes.
///
/// Returns the [`LocalAction`] IR node and any non-fatal diagnostics (unrecognised
/// ravelact verbs, dangling annotation references, trailing comments, etc.).
pub fn parse_action(
    action_yml_path: &Path,
    root: &Path,
) -> Result<(LocalAction, Vec<ParseDiagnostic>)> {
    let raw = std::fs::read_to_string(action_yml_path)
        .with_context(|| format!("read action {}", action_yml_path.display()))?;
    let mut docs = MarkedYaml::load_from_str(&raw)
        .with_context(|| format!("parse YAML {}", action_yml_path.display()))?;
    let doc = docs
        .pop()
        .ok_or_else(|| anyhow::anyhow!("empty YAML document: {}", action_yml_path.display()))?;

    let dir = action_yml_path
        .parent()
        .context("action.yml has no parent directory")?;
    let rel_dir = dir
        .strip_prefix(root)
        .with_context(|| format!("action {} not under root {}", dir.display(), root.display()))?;
    let id_string = if rel_dir.as_os_str().is_empty() {
        ".".to_string()
    } else {
        path_to_forward(rel_dir)
    };
    let id = ActionId(id_string);

    let source = SourcePos {
        file: action_yml_path.to_path_buf(),
        line: Some(doc.span.start.line()),
    };

    let name = get_field(&doc, "name")
        .and_then(as_str)
        .map(|s| s.to_string());

    let inputs = parse_input_decls(get_field(&doc, "inputs"));
    let outputs = parse_output_decls(get_field(&doc, "outputs"));

    let runs = get_field(&doc, "runs").ok_or_else(|| {
        anyhow::anyhow!("action.yml missing `runs`: {}", action_yml_path.display())
    })?;
    let using = get_field(runs, "using").and_then(as_str).ok_or_else(|| {
        anyhow::anyhow!(
            "action.yml `runs.using` missing: {}",
            action_yml_path.display()
        )
    })?;

    // Diagnostics collected during parsing (malformed `uses:`, unknown ravelact
    // verbs, dangling annotation references, ...). Initialised before
    // composite-step parsing so step-level diagnostics can be appended in place.
    let mut diagnostics: Vec<ParseDiagnostic> = Vec::new();

    let (kind, steps) = match using {
        "composite" => {
            let steps = parse_steps(get_field(runs, "steps"), action_yml_path, &mut diagnostics);
            (ActionKind::Composite, steps)
        }
        v if v.starts_with("node") => (
            ActionKind::JavaScript {
                node_version: v.to_string(),
            },
            Vec::new(),
        ),
        "docker" => (ActionKind::Docker, Vec::new()),
        other => {
            return Err(anyhow::anyhow!(
                "unknown runs.using `{other}` in {}",
                action_yml_path.display()
            ));
        }
    };

    let mut action = LocalAction {
        id,
        source,
        name,
        kind,
        inputs,
        outputs,
        steps,
        annotations: Vec::new(),
    };

    // Scan for ravelact annotations in the action source, mirroring the workflow
    // parser's annotation handling. `diagnostics` was created earlier so step
    // parsing could append to it; annotation scanning extends the same vec.
    let mut scalar_ranges: Vec<(usize, usize)> = Vec::new();
    collect_block_scalar_ranges(&doc, &mut scalar_ranges);
    let raws = scan_ravelact_comments(&raw, action_yml_path, &scalar_ranges, &mut diagnostics);
    attach_local_action_annotations(&mut action, raws, &mut diagnostics);

    Ok((action, diagnostics))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }

    #[test]
    fn parses_composite_action() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/setup/action.yml");
        write(
            &yml,
            r#"
name: Setup
description: setup steps
inputs:
  toolchain:
    required: true
    description: rust toolchain
outputs:
  cache_hit:
    description: was cache hit?
runs:
  using: composite
  steps:
    - name: noop
      run: echo hi
      shell: bash
    - uses: actions/checkout@v4
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty(), "no ravelact comments => no diagnostics");
        assert_eq!(action.id.0, ".github/actions/setup");
        assert!(matches!(action.kind, ActionKind::Composite));
        assert!(action.source.line.is_some(), "saphyr should populate line");
        assert_eq!(action.inputs.len(), 1);
        assert_eq!(action.inputs[0].name, "toolchain");
        assert!(action.inputs[0].required);
        assert_eq!(action.outputs.len(), 1);
        assert_eq!(action.steps.len(), 2);
        assert_eq!(action.steps[0].run.as_deref(), Some("echo hi"));
        match action.steps[1].uses.as_ref().unwrap() {
            UsesRef::External { owner, repo, .. } => {
                assert_eq!(owner, "actions");
                assert_eq!(repo, "checkout");
            }
            other => panic!("expected External, got {other:?}"),
        }
    }

    #[test]
    fn parses_javascript_action() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/js/action.yml");
        write(
            &yml,
            r#"
name: JS
runs:
  using: node20
  main: index.js
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty(), "no ravelact comments => no diagnostics");
        assert!(action.source.line.is_some(), "saphyr should populate line");
        match action.kind {
            ActionKind::JavaScript { node_version } => assert_eq!(node_version, "node20"),
            other => panic!("expected JavaScript, got {other:?}"),
        }
        assert!(action.steps.is_empty());
    }

    #[test]
    fn composite_action_annotation_attaches_to_step() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/notify/action.yml");
        // The annotation comment must appear BEFORE the step mapping start
        // (i.e. before the `- run:` line) so that `attach_composite_annotations`
        // anchors it to the step (smallest source.line > comment line).
        write(
            &yml,
            r#"
name: Notify
runs:
  using: composite
  steps:
    # ravelact:dispatches scripts/bad-target.sh
    - run: gh workflow run build.yaml
      shell: bash
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        // The annotation target `scripts/bad-target.sh` is dangling (not under
        // .github/workflows/ and not a .yml/.yaml). Expect 1 dangling diagnostic.
        assert_eq!(
            diags.len(),
            1,
            "expected one dangling diagnostic, got: {diags:?}"
        );
        let step = &action.steps[0];
        assert_eq!(
            step.annotations.len(),
            1,
            "annotation should attach to the step"
        );
        assert!(matches!(
            step.annotations[0].resolution,
            AnnotationResolution::Dangling { .. }
        ));
    }

    /// A composite-action step with a malformed `uses:` (missing `@ref`) must
    /// surface a `ParseDiagnostic` rather than silently produce `uses: None`.
    /// Regression test for issue #112.
    #[test]
    fn composite_step_uses_missing_ref_emits_diagnostic() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/bad/action.yml");
        // Line numbers (1-based) within the raw literal:
        //   1: (leading newline)
        //   2: name: Bad
        //   3: runs:
        //   4:   using: composite
        //   5:   steps:
        //   6:     - uses: actions/checkout    <-- diagnostic anchor
        write(
            &yml,
            r#"
name: Bad
runs:
  using: composite
  steps:
    - uses: actions/checkout
"#,
        );

        let (action, diags) = parse_action(&yml, root).unwrap();

        assert_eq!(
            diags.len(),
            1,
            "expected exactly one ParseDiagnostic, got: {diags:?}"
        );
        let d = &diags[0];
        assert_eq!(d.file, yml, "diagnostic file should match the action.yml");
        assert_eq!(
            d.line, 6,
            "diagnostic should pin the line of the `uses:` value"
        );
        assert!(
            d.message.contains("uses"),
            "message should mention `uses`: {}",
            d.message
        );

        // The Step itself is preserved (so step indices stay stable) but its
        // `uses` field is `None` because the value failed to parse.
        assert_eq!(action.steps.len(), 1, "step is still in the IR");
        assert!(
            action.steps[0].uses.is_none(),
            "malformed uses should be None"
        );
    }

    /// `runs.using: node12` is a legacy JS runtime; the parser must accept any
    /// `node*` value verbatim and stash it in `ActionKind::JavaScript`.
    #[test]
    fn parses_node12_javascript_action() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/legacy/action.yml");
        write(
            &yml,
            r#"
name: Legacy
runs:
  using: node12
  main: index.js
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty());
        match action.kind {
            ActionKind::JavaScript { node_version } => assert_eq!(node_version, "node12"),
            other => panic!("expected JavaScript, got {other:?}"),
        }
        assert!(action.steps.is_empty(), "JS action has no composite steps");
    }

    #[test]
    fn parses_node16_javascript_action() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/n16/action.yml");
        write(
            &yml,
            r#"
name: N16
runs:
  using: node16
  main: index.js
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty());
        match action.kind {
            ActionKind::JavaScript { node_version } => assert_eq!(node_version, "node16"),
            other => panic!("expected JavaScript, got {other:?}"),
        }
    }

    /// Forward-looking node runtime. The `node*` prefix dispatch should not be
    /// version-gated, so a future `node24` parses without source changes.
    #[test]
    fn parses_node24_javascript_action() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/n24/action.yml");
        write(
            &yml,
            r#"
name: N24
runs:
  using: node24
  main: index.js
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty());
        match action.kind {
            ActionKind::JavaScript { node_version } => assert_eq!(node_version, "node24"),
            other => panic!("expected JavaScript, got {other:?}"),
        }
    }

    #[test]
    fn parses_docker_action() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/dock/action.yml");
        write(
            &yml,
            r#"
name: Dock
runs:
  using: docker
  image: Dockerfile
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty());
        assert!(matches!(action.kind, ActionKind::Docker));
        assert!(action.steps.is_empty(), "docker action has no IR steps");
    }

    /// `using: composite` without a `steps:` key is degenerate but legal YAML;
    /// the parser must still classify it as Composite and produce zero steps.
    #[test]
    fn parses_composite_action_without_steps() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/empty/action.yml");
        write(
            &yml,
            r#"
name: Empty
runs:
  using: composite
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty());
        assert!(matches!(action.kind, ActionKind::Composite));
        assert!(action.steps.is_empty(), "missing steps => empty Vec");
    }

    /// Action manifest at a non-standard path (not under `.github/actions/`):
    /// the parser is path-agnostic — only `root` matters for ID derivation.
    #[test]
    fn parses_action_at_non_standard_path() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join("tools/local-action/action.yml");
        write(
            &yml,
            r#"
name: Tool
runs:
  using: composite
  steps:
    - run: echo hello
      shell: bash
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty());
        assert_eq!(action.id.0, "tools/local-action");
        assert_eq!(action.steps.len(), 1);
    }

    /// Manifest sitting directly under the root (no parent directory) gets the
    /// sentinel `"."` id.
    #[test]
    fn parses_action_at_root_uses_dot_id() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join("action.yml");
        write(
            &yml,
            r#"
name: Root
runs:
  using: composite
  steps: []
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty());
        assert_eq!(action.id.0, ".", "root-level action gets `.` id");
    }

    // -- negative cases -----------------------------------------------------

    #[test]
    fn errors_when_runs_missing() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/no-runs/action.yml");
        write(
            &yml,
            r#"
name: NoRuns
description: missing runs key
"#,
        );
        let err = parse_action(&yml, root).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("missing `runs`"), "got: {msg}");
    }

    /// `runs: ~` (explicit YAML null) — the `runs` key is present but cannot be
    /// indexed for `using:`, so the parser bails on the `using` lookup.
    #[test]
    fn errors_when_runs_is_null() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/null-runs/action.yml");
        write(
            &yml,
            r#"
name: NullRuns
runs: ~
"#,
        );
        let err = parse_action(&yml, root).unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("`runs.using` missing"),
            "expected runs.using missing error, got: {msg}"
        );
    }

    #[test]
    fn errors_when_runs_using_unknown() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/weird/action.yml");
        write(
            &yml,
            r#"
name: Weird
runs:
  using: shell
"#,
        );
        let err = parse_action(&yml, root).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("unknown runs.using `shell`"), "got: {msg}");
    }

    /// `parse_action` does not gate on file extension — the caller (discovery)
    /// is responsible for selecting `action.yml` / `action.yaml`. Feeding a
    /// well-formed YAML through a `.txt` path still parses successfully.
    #[test]
    fn extension_is_not_validated_by_parser() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let yml = root.join(".github/actions/odd/action.txt");
        write(
            &yml,
            r#"
name: Odd
runs:
  using: composite
  steps: []
"#,
        );
        let (action, diags) = parse_action(&yml, root).unwrap();
        assert!(diags.is_empty());
        assert!(matches!(action.kind, ActionKind::Composite));
    }
}
