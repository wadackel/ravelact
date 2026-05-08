use crate::ir::*;
use crate::parser::action::parse_action;
use crate::parser::workflow::parse_workflow;
use anyhow::{Context, Result};
use globset::GlobSet;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

const SCHEMA_VERSION: u32 = 10;

// Pruned at any depth, not just the repository root: these names denote
// derived / VCS-internal artefacts whose semantics are depth-independent
// (e.g. nested `pkg/foo/dist/action.yml` is bundled output, not a separate
// local-action source).
const EXCLUDED_DIR_NAMES: &[&str] = &[".git", "target", "node_modules", "dist", "build"];

#[derive(Debug, Clone)]
pub struct SourceInventory {
    pub root: PathBuf,
    pub workflow_files: Vec<PathBuf>,
    pub action_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebuildStats {
    pub reused_workflows: usize,
    pub reparsed_workflows: usize,
    pub reused_actions: usize,
    pub reparsed_actions: usize,
}

#[derive(Debug, Clone)]
pub struct RebuildResult {
    pub ir: Ir,
    pub stats: RebuildStats,
    /// Non-fatal diagnostics surfaced by `parse_workflow` for files that were
    /// reparsed (not reused from cache). Cached workflows do not contribute
    /// fresh diagnostics — their annotations live on the IR itself, so `wiring`
    /// can still re-derive Dangling findings from the cached IR.
    pub diagnostics: Vec<ParseDiagnostic>,
}

pub fn current_schema_version() -> u32 {
    SCHEMA_VERSION
}

/// Build the full IR rooted at `root`. Diagnostics are dropped here; callers
/// that need them should go through `rebuild_ir_from_inventory` directly (or
/// `cache::load_or_build`).
///
/// `excludes` filters local-action manifests by their workspace-relative path.
/// Workflow files under `.github/workflows/` are not affected — see
/// `collect_action_files` for the rationale.
pub fn build_ir(root: &Path, excludes: &GlobSet) -> Result<Ir> {
    let inventory = discover_sources(root, excludes)?;
    Ok(rebuild_ir_from_inventory(&inventory, None, &BTreeSet::new())?.ir)
}

pub fn discover_sources(root: &Path, excludes: &GlobSet) -> Result<SourceInventory> {
    let root_canonical = root
        .canonicalize()
        .with_context(|| format!("canonicalize root {}", root.display()))?;
    Ok(SourceInventory {
        workflow_files: collect_workflow_files(&root_canonical)?,
        action_files: collect_action_files(&root_canonical, excludes)?,
        root: root_canonical,
    })
}

pub fn rebuild_ir_from_inventory(
    inventory: &SourceInventory,
    cached: Option<&Ir>,
    stale_sources: &BTreeSet<PathBuf>,
) -> Result<RebuildResult> {
    let cached_workflows = cached
        .map(|ir| {
            ir.workflows
                .iter()
                .cloned()
                .map(|wf| (wf.source.file.clone(), wf))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let cached_actions = cached
        .map(|ir| {
            ir.actions
                .iter()
                .cloned()
                .map(|c| (c.source.file.clone(), c))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    let mut stats = RebuildStats::default();
    let mut diagnostics: Vec<ParseDiagnostic> = Vec::new();
    let mut workflows = Vec::new();
    for path in &inventory.workflow_files {
        if !stale_sources.contains(path) {
            if let Some(wf) = cached_workflows.get(path) {
                stats.reused_workflows += 1;
                workflows.push(wf.clone());
                continue;
            }
        }

        let (wf, diags) = parse_workflow(path, &inventory.root)
            .with_context(|| format!("parse workflow {}", path.display()))?;
        stats.reparsed_workflows += 1;
        diagnostics.extend(diags);
        workflows.push(wf);
    }

    let mut actions = Vec::new();
    for path in &inventory.action_files {
        if !stale_sources.contains(path) {
            if let Some(action) = cached_actions.get(path) {
                stats.reused_actions += 1;
                actions.push(action.clone());
                continue;
            }
        }

        match parse_action(path, &inventory.root) {
            Ok((action, diags)) => {
                stats.reparsed_actions += 1;
                diagnostics.extend(diags);
                actions.push(action);
            }
            Err(err) => eprintln!("warn: parse action {}: {err}", path.display()),
        }
    }

    workflows.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    actions.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    let external_actions = collect_externals(&workflows, &actions);

    Ok(RebuildResult {
        ir: Ir {
            schema_version: SCHEMA_VERSION,
            root: inventory.root.clone(),
            workflows,
            actions,
            external_actions,
        },
        stats,
        diagnostics,
    })
}

fn collect_workflow_files(root: &Path) -> Result<Vec<PathBuf>> {
    let workflows_dir = root.join(".github").join("workflows");
    if !workflows_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&workflows_dir)
        .with_context(|| format!("read_dir {}", workflows_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yml" && ext != "yaml" {
            continue;
        }
        out.push(path);
    }
    out.sort();
    Ok(out)
}

fn collect_action_files(root: &Path, excludes: &GlobSet) -> Result<Vec<PathBuf>> {
    let mut candidates = Vec::new();
    // filter_entry returning false prunes the subtree, so excluded directories
    // are not descended into.
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !path_has_excluded_segment(root, e.path()))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                eprintln!("warn: walkdir: {err}");
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name != "action.yml" && name != "action.yaml" {
            continue;
        }
        candidates.push(entry.path().to_path_buf());
    }

    candidates.sort();
    let mut by_dir: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    for path in candidates {
        let Some(dir) = path.parent() else {
            continue;
        };
        by_dir.entry(dir.to_path_buf()).or_insert(path);
    }

    // User-supplied `--exclude` globs are applied at file-collection time on the
    // workspace-relative path. Patterns like `tests/**` would not match the
    // `tests` directory entry itself, so prune at filter_entry time is not an
    // option; filtering after dedup-by-dir is sufficient at the scale of typical
    // repos.
    let out = by_dir
        .into_values()
        .filter(|path| match path.strip_prefix(root) {
            Ok(rel) => !excludes.is_match(rel),
            Err(_) => true,
        })
        .collect::<Vec<_>>();
    Ok(out)
}

fn path_has_excluded_segment(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    rel.components().any(|c| match c {
        Component::Normal(name) => {
            let s = name.to_string_lossy();
            EXCLUDED_DIR_NAMES.contains(&s.as_ref())
        }
        _ => false,
    })
}

fn collect_externals(workflows: &[Workflow], actions: &[LocalAction]) -> Vec<ExternalActionRef> {
    let mut set: BTreeSet<ExternalActionRef> = BTreeSet::new();
    for wf in workflows {
        for job in &wf.jobs {
            if let Some(call) = &job.calls_workflow {
                if let WorkflowRef::External {
                    owner,
                    repo,
                    path,
                    gitref,
                } = &call.workflow_ref
                {
                    set.insert(ExternalActionRef {
                        owner: owner.clone(),
                        repo: repo.clone(),
                        subpath: if path.is_empty() {
                            None
                        } else {
                            Some(path.clone())
                        },
                        gitref: gitref.clone(),
                    });
                }
            }
            for step in &job.steps {
                push_step_external(step, &mut set);
            }
        }
    }
    for action in actions {
        for step in &action.steps {
            push_step_external(step, &mut set);
        }
    }
    set.into_iter().collect()
}

fn push_step_external(step: &Step, set: &mut BTreeSet<ExternalActionRef>) {
    if let Some(UsesRef::External {
        owner,
        repo,
        subpath,
        gitref,
    }) = &step.uses
    {
        set.insert(ExternalActionRef {
            owner: owner.clone(),
            repo: repo.clone(),
            subpath: subpath.clone(),
            gitref: gitref.clone(),
        });
    }
}

// Implement Ord for ExternalActionRef so it can live in BTreeSet.
impl Ord for ExternalActionRef {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (
            self.owner.as_str(),
            self.repo.as_str(),
            self.subpath.as_deref().unwrap_or(""),
            self.gitref.as_str(),
        )
            .cmp(&(
                other.owner.as_str(),
                other.repo.as_str(),
                other.subpath.as_deref().unwrap_or(""),
                other.gitref.as_str(),
            ))
    }
}
impl PartialOrd for ExternalActionRef {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use globset::{Glob, GlobSetBuilder};
    use std::io::Write;
    use tempfile::tempdir;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::File::create(path)
            .unwrap()
            .write_all(content.as_bytes())
            .unwrap();
    }

    fn glob_set(patterns: &[&str]) -> GlobSet {
        let mut builder = GlobSetBuilder::new();
        for pat in patterns {
            builder.add(Glob::new(pat).unwrap());
        }
        builder.build().unwrap()
    }

    #[test]
    fn discovers_workflows_and_local_actions() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        write(
            &root.join(".github/workflows/ci.yml"),
            "name: CI\non: push\njobs:\n  t:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n",
        );
        write(
            &root.join(".github/workflows/build.yml"),
            "on:\n  workflow_call:\njobs:\n  b:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: ./.github/actions/setup\n",
        );
        write(
            &root.join(".github/actions/setup/action.yml"),
            "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
        );
        write(
            &root.join("target/should-be-ignored/action.yml"),
            "runs:\n  using: composite\n  steps: []\n",
        );

        let ir = build_ir(root, &GlobSet::empty()).unwrap();
        assert_eq!(ir.workflows.len(), 2);
        assert_eq!(ir.actions.len(), 1);
        assert_eq!(ir.actions[0].id.0, ".github/actions/setup");
        assert!(
            ir.workflows.iter().all(|w| w.source.line.is_some()),
            "saphyr should populate source.line on all workflows",
        );
        assert!(
            ir.actions.iter().all(|a| a.source.line.is_some()),
            "saphyr should populate source.line on all local actions",
        );
        assert!(
            ir.external_actions
                .iter()
                .any(|e| e.owner == "actions" && e.repo == "checkout"),
            "expected actions/checkout in externals: {:?}",
            ir.external_actions
        );
    }

    #[test]
    fn excludes_nested_dir_names() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // canonical composite (should be detected)
        write(
            &root.join("pkg/foo/action.yml"),
            "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
        );
        // nested dist/ — should be pruned
        write(
            &root.join("pkg/foo/dist/should-be-ignored/action.yml"),
            "runs:\n  using: composite\n  steps: []\n",
        );
        // nested node_modules/ — pruning must not be specific to "dist"
        write(
            &root.join("apps/bar/node_modules/whatever/action.yml"),
            "runs:\n  using: composite\n  steps: []\n",
        );

        let ir = build_ir(root, &GlobSet::empty()).unwrap();
        assert_eq!(ir.actions.len(), 1);
        assert_eq!(ir.actions[0].id.0, "pkg/foo");
    }

    #[test]
    fn path_has_excluded_segment_recognizes_all_names() {
        let root = Path::new("/repo");
        // Every name in EXCLUDED_DIR_NAMES must be recognized at any depth.
        for name in EXCLUDED_DIR_NAMES {
            let nested = root.join("a").join("b").join(name).join("c").join("file");
            assert!(
                path_has_excluded_segment(root, &nested),
                "expected nested {name:?} to be excluded: {}",
                nested.display(),
            );
            let top = root.join(name).join("file");
            assert!(
                path_has_excluded_segment(root, &top),
                "expected top-level {name:?} to be excluded: {}",
                top.display(),
            );
        }
        // A path under a non-excluded directory must not be flagged.
        assert!(!path_has_excluded_segment(root, &root.join("src/foo.rs")));
        // Substring containment must not match — only full path components.
        assert!(!path_has_excluded_segment(
            root,
            &root.join("dist-tools/action.yml"),
        ));
        assert!(!path_has_excluded_segment(
            root,
            &root.join("my-target/action.yml"),
        ));
        // Path outside `root` (strip_prefix fails) must be reported as not-excluded.
        assert!(!path_has_excluded_segment(
            root,
            Path::new("/elsewhere/dist/action.yml"),
        ));
    }

    #[test]
    fn accepts_both_yml_and_yaml_extensions() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Mix of `.yml` and `.yaml` for both workflow and local action.
        write(
            &root.join(".github/workflows/ci.yml"),
            "name: CI\non: push\njobs:\n  t:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
        );
        write(
            &root.join(".github/workflows/release.yaml"),
            "name: Release\non: push\njobs:\n  r:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
        );
        write(
            &root.join(".github/actions/setup-yml/action.yml"),
            "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
        );
        write(
            &root.join(".github/actions/setup-yaml/action.yaml"),
            "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
        );

        let ir = build_ir(root, &GlobSet::empty()).unwrap();
        assert_eq!(
            ir.workflows.len(),
            2,
            "both .yml and .yaml workflows must be discovered"
        );
        assert_eq!(
            ir.actions.len(),
            2,
            "both .yml and .yaml local actions must be discovered"
        );
    }

    #[test]
    fn workflow_classifier_ignores_subdirs_of_workflows() {
        // Workflow files are only valid directly under `.github/workflows/`.
        // Files in subdirectories (e.g. `.github/workflows/templates/foo.yml`)
        // are not workflows per GitHub Actions semantics.
        let dir = tempdir().unwrap();
        let root = dir.path();

        write(
            &root.join(".github/workflows/ci.yml"),
            "name: CI\non: push\njobs:\n  t:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
        );
        // This file lives in a subdirectory of `.github/workflows/`. GitHub
        // Actions does not treat it as a workflow, and neither should we.
        write(
            &root.join(".github/workflows/templates/partial.yml"),
            "this: is\nnot: a workflow\n",
        );

        let ir = build_ir(root, &GlobSet::empty()).unwrap();
        assert_eq!(ir.workflows.len(), 1);
        assert!(
            ir.workflows[0]
                .source
                .file
                .ends_with(".github/workflows/ci.yml"),
            "only the top-level workflow should be discovered, got: {:?}",
            ir.workflows[0].source.file,
        );
    }

    #[test]
    fn discovers_with_user_exclude_glob() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // canonical workflow + canonical action (should always be detected)
        write(
            &root.join(".github/workflows/ci.yml"),
            "name: CI\non: push\njobs:\n  t:\n    runs-on: ubuntu-latest\n    steps:\n      - run: echo hi\n",
        );
        write(
            &root.join(".github/actions/setup/action.yml"),
            "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
        );
        // test fixture action (should be excluded by `tests/**`)
        write(
            &root.join("tests/fixtures/foo/action.yml"),
            "runs:\n  using: composite\n  steps:\n    - run: echo hi\n      shell: bash\n",
        );

        // Without exclude: both actions discovered.
        let ir_all = build_ir(root, &GlobSet::empty()).unwrap();
        assert_eq!(ir_all.workflows.len(), 1);
        assert_eq!(ir_all.actions.len(), 2);

        // With `tests/**` exclude: only the canonical action remains, but the
        // workflow is unaffected (workflow files are not subject to --exclude).
        let ir_filtered = build_ir(root, &glob_set(&["tests/**"])).unwrap();
        assert_eq!(ir_filtered.workflows.len(), 1);
        assert_eq!(ir_filtered.actions.len(), 1);
        assert_eq!(ir_filtered.actions[0].id.0, ".github/actions/setup");
    }
}
