//! Query-layer helper: name → WorkflowId reverse index for `workflow_run` resolution.
//!
//! Per the GitHub Actions spec (Events that trigger workflows — workflow_run section),
//! `workflow_run.workflows: [Foo]` matches by the **display name** (`name:` field) of the
//! target workflow. When a workflow omits `name:`, GitHub Actions uses the relative path
//! (e.g. `.github/workflows/build.yml`) as the effective name.
//!
//! Name collisions (multiple workflows sharing the same `name:`) are allowed; the index
//! therefore maps one name to a `Vec<WorkflowId>`.
//!
//! Spec source: Events that trigger workflows —
//! https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows

use crate::ir::{EventExtras, EventKind, Ir, WorkflowId};
use std::collections::BTreeMap;

/// A reverse index from workflow display name → matching `WorkflowId`s.
///
/// Built lazily from an `Ir` reference via [`build_index`]. Multiple workflows
/// may share the same display name (collisions are permitted by GA), hence the
/// `Vec<WorkflowId>` value.
pub type WorkflowRunIndex<'ir> = BTreeMap<String, Vec<&'ir WorkflowId>>;

/// One entry in a partition result: `(declaring_workflow_id, name_string)`.
pub type WorkflowRunNameRef<'ir> = (&'ir WorkflowId, &'ir str);

/// Build a name → `WorkflowId` reverse index for `workflow_run` resolution.
///
/// For each workflow in `ir`:
/// - If `workflow.name` is `Some(n)`, the entry key is `n`.
/// - If `workflow.name` is `None`, the entry key is `workflow.id.0` (the
///   relative path), matching GitHub Actions' default-name behaviour.
///
/// The returned map is a `BTreeMap` so iteration order is deterministic.
pub fn build_index(ir: &Ir) -> WorkflowRunIndex<'_> {
    let mut index: WorkflowRunIndex<'_> = BTreeMap::new();
    for wf in &ir.workflows {
        let key = wf.name.as_deref().unwrap_or(&wf.id.0).to_string();
        index.entry(key).or_default().push(&wf.id);
    }
    index
}

/// Resolve a single `workflow_run.workflows` name string to all matching local
/// `WorkflowId`s. Returns an empty slice when the name is unresolvable (no
/// local workflow has that display name or path).
///
/// This is the canonical resolution helper; callers in `mermaid`, `wiring`, and
/// `trace` should call this rather than re-implementing name matching.
pub fn resolve_name<'idx>(
    index: &'idx WorkflowRunIndex<'_>,
    name: &str,
) -> &'idx [&'idx WorkflowId] {
    index.get(name).map(|v| v.as_slice()).unwrap_or_default()
}

/// Returns `true` when the given `workflow_run.workflows` name resolves to at
/// least one local workflow in `index`.
pub fn is_resolvable(index: &WorkflowRunIndex<'_>, name: &str) -> bool {
    !resolve_name(index, name).is_empty()
}

/// Collect all `workflow_run.workflows` name strings declared across every
/// `workflow_run` trigger in `ir`. Returns `(resolvable, dangling)` partitions.
///
/// `dangling` entries are `(declaring_workflow_id, unresolvable_name)` pairs
/// suitable for wiring reporting.
pub fn partition_workflow_run_names<'ir>(
    ir: &'ir Ir,
    index: &WorkflowRunIndex<'ir>,
) -> (Vec<WorkflowRunNameRef<'ir>>, Vec<WorkflowRunNameRef<'ir>>) {
    let mut resolved: Vec<(&'ir WorkflowId, &'ir str)> = Vec::new();
    let mut dangling: Vec<(&'ir WorkflowId, &'ir str)> = Vec::new();

    for wf in &ir.workflows {
        for trigger in &wf.triggers {
            if trigger.event != EventKind::WorkflowRun {
                continue;
            }
            if let Some(EventExtras::WorkflowRun { workflows }) = &trigger.extras {
                for name in workflows {
                    if is_resolvable(index, name) {
                        resolved.push((&wf.id, name.as_str()));
                    } else {
                        dangling.push((&wf.id, name.as_str()));
                    }
                }
            }
        }
    }

    (resolved, dangling)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{EventKind, TriggerSpec, Workflow, WorkflowId};
    use crate::ir::{Ir, SourcePos};
    use std::path::PathBuf;

    fn make_ir(workflows: Vec<Workflow>) -> Ir {
        Ir {
            schema_version: 3,
            root: PathBuf::from("/tmp/test"),
            workflows,
            actions: vec![],
            external_actions: vec![],
        }
    }

    fn wf_named(id: &str, name: Option<&str>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: name.map(|s| s.to_string()),
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: vec![],
        }
    }

    fn wf_workflow_run(id: &str, name: Option<&str>, upstream_names: Vec<&str>) -> Workflow {
        use crate::ir::{EventExtras, RefFilter};
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: name.map(|s| s.to_string()),
            run_name: None,
            triggers: vec![crate::ir::TriggerSpec {
                event: EventKind::WorkflowRun,
                branches: RefFilter::None,
                tags: RefFilter::None,
                paths: RefFilter::None,
                types: None,
                extras: Some(EventExtras::WorkflowRun {
                    workflows: upstream_names.into_iter().map(|s| s.to_string()).collect(),
                }),
            }],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: vec![],
        }
    }

    #[test]
    fn build_index_uses_name_field() {
        let ir = make_ir(vec![wf_named(".github/workflows/ci.yml", Some("CI"))]);
        let idx = build_index(&ir);
        assert!(
            idx.contains_key("CI"),
            "index must contain display name key"
        );
        assert!(
            !idx.contains_key(".github/workflows/ci.yml"),
            "path key must not appear when name is set"
        );
    }

    #[test]
    fn build_index_falls_back_to_path_when_no_name() {
        let ir = make_ir(vec![wf_named(".github/workflows/ci.yml", None)]);
        let idx = build_index(&ir);
        assert!(
            idx.contains_key(".github/workflows/ci.yml"),
            "path key must be present as fallback"
        );
    }

    #[test]
    fn build_index_handles_name_collision() {
        let ir = make_ir(vec![
            wf_named(".github/workflows/a.yml", Some("CI")),
            wf_named(".github/workflows/b.yml", Some("CI")),
        ]);
        let idx = build_index(&ir);
        assert_eq!(
            idx["CI"].len(),
            2,
            "both workflows should be in the CI bucket"
        );
    }

    #[test]
    fn resolve_name_returns_matching_ids() {
        let ir = make_ir(vec![wf_named(".github/workflows/build.yml", Some("Build"))]);
        let idx = build_index(&ir);
        let matches = resolve_name(&idx, "Build");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].0, ".github/workflows/build.yml");
    }

    #[test]
    fn resolve_name_returns_empty_for_unknown() {
        let ir = make_ir(vec![wf_named(".github/workflows/build.yml", Some("Build"))]);
        let idx = build_index(&ir);
        assert!(resolve_name(&idx, "Nonexistent").is_empty());
    }

    #[test]
    fn is_resolvable_true_for_known_name() {
        let ir = make_ir(vec![wf_named(".github/workflows/ci.yml", Some("CI"))]);
        let idx = build_index(&ir);
        assert!(is_resolvable(&idx, "CI"));
    }

    #[test]
    fn is_resolvable_false_for_unknown() {
        let ir = make_ir(vec![wf_named(".github/workflows/ci.yml", Some("CI"))]);
        let idx = build_index(&ir);
        assert!(!is_resolvable(&idx, "Missing"));
    }

    #[test]
    fn partition_separates_resolved_and_dangling() {
        let downstream_id = ".github/workflows/downstream.yml";
        let ir = make_ir(vec![
            wf_named(".github/workflows/trigger.yml", Some("Trigger")),
            wf_workflow_run(downstream_id, Some("Downstream"), vec!["Trigger", "Ghost"]),
        ]);
        let idx = build_index(&ir);
        let (resolved, dangling) = partition_workflow_run_names(&ir, &idx);

        assert_eq!(resolved.len(), 1, "only Trigger resolves");
        assert_eq!(resolved[0].1, "Trigger");

        assert_eq!(dangling.len(), 1, "Ghost is dangling");
        assert_eq!(dangling[0].1, "Ghost");
        assert_eq!(dangling[0].0 .0, downstream_id);
    }

    #[test]
    fn resolve_name_via_path_fallback() {
        // When upstream has no name:, the path acts as the key.
        let upstream_path = ".github/workflows/nightly.yml";
        let ir = make_ir(vec![
            wf_named(upstream_path, None),
            wf_workflow_run(".github/workflows/consumer.yml", None, vec![upstream_path]),
        ]);
        let idx = build_index(&ir);
        let matches = resolve_name(&idx, upstream_path);
        assert_eq!(matches.len(), 1, "path fallback must resolve");
        assert_eq!(matches[0].0, upstream_path);
    }
}
