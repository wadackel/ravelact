use crate::ir::Ir;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerSummary {
    pub event: String,
    pub entry_workflows: usize,
    pub declarations: usize,
    pub typed: usize,
    pub filtered: usize,
    pub examples: Vec<String>,
}

#[derive(Debug, Default)]
struct TriggerAccumulator {
    entry_workflows: BTreeSet<String>,
    declarations: usize,
    typed: usize,
    filtered: usize,
    examples: BTreeSet<String>,
}

/// Summarize trigger declarations across the workflow estate.
///
/// Counts intentionally follow the CLI contract from issue #219:
/// - `entry_workflows` is a unique workflow count and excludes `workflow_call`.
/// - `declarations`, `typed`, and `filtered` count trigger declarations.
/// - examples are unique workflow paths, sorted and capped for large estates.
pub fn triggers(ir: &Ir) -> Vec<TriggerSummary> {
    let mut by_event: BTreeMap<String, TriggerAccumulator> = BTreeMap::new();

    for workflow in &ir.workflows {
        for trigger in &workflow.triggers {
            let acc = by_event
                .entry(trigger.event.name().to_string())
                .or_default();
            acc.declarations += 1;
            acc.examples.insert(workflow.id.0.clone());
            if trigger.is_entry_point() {
                acc.entry_workflows.insert(workflow.id.0.clone());
            }
            if trigger.types.is_some() {
                acc.typed += 1;
            }
            if !trigger.branches.is_none() || !trigger.tags.is_none() || !trigger.paths.is_none() {
                acc.filtered += 1;
            }
        }
    }

    let mut rows: Vec<TriggerSummary> = by_event
        .into_iter()
        .map(|(event, acc)| TriggerSummary {
            event,
            entry_workflows: acc.entry_workflows.len(),
            declarations: acc.declarations,
            typed: acc.typed,
            filtered: acc.filtered,
            examples: acc.examples.into_iter().take(3).collect(),
        })
        .collect();
    rows.sort_by(|a, b| {
        b.entry_workflows
            .cmp(&a.entry_workflows)
            .then_with(|| a.event.cmp(&b.event))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{EventKind, RefFilter, SourcePos, TriggerSpec, Workflow, WorkflowId};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn ir(workflows: Vec<Workflow>) -> Ir {
        Ir {
            schema_version: 1,
            root: PathBuf::new(),
            workflows,
            actions: Vec::new(),
            external_actions: Vec::new(),
        }
    }

    fn workflow(id: &str, triggers: Vec<TriggerSpec>) -> Workflow {
        Workflow {
            id: WorkflowId(id.to_string()),
            source: SourcePos {
                file: PathBuf::from(id),
                line: None,
            },
            name: None,
            run_name: None,
            triggers,
            jobs: Vec::new(),
            permissions: None,
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            annotations: Vec::new(),
        }
    }

    fn trigger(event: EventKind) -> TriggerSpec {
        TriggerSpec {
            event,
            branches: RefFilter::None,
            tags: RefFilter::None,
            paths: RefFilter::None,
            types: None,
            extras: None,
        }
    }

    fn include_filter(pattern: &str) -> RefFilter {
        RefFilter::Include {
            patterns: vec![pattern.to_string()],
        }
    }

    #[test]
    fn summarizes_counts_and_sorts_by_entry_workflows_then_event() {
        let mut typed_pr = trigger(EventKind::PullRequest);
        typed_pr.types = Some(vec!["opened".to_string()]);
        typed_pr.paths = include_filter("src/**");

        let mut filtered_push = trigger(EventKind::Push);
        filtered_push.branches = include_filter("main");

        let rows = triggers(&ir(vec![
            workflow(".github/workflows/a.yml", vec![trigger(EventKind::Push)]),
            workflow(".github/workflows/b.yml", vec![filtered_push]),
            workflow(
                ".github/workflows/c.yml",
                vec![trigger(EventKind::WorkflowCall)],
            ),
            workflow(".github/workflows/d.yml", vec![typed_pr]),
            workflow(
                ".github/workflows/e.yml",
                vec![trigger(EventKind::Other {
                    name: "z_future".to_string(),
                })],
            ),
        ]));

        assert_eq!(
            rows.iter()
                .map(|row| row.event.as_str())
                .collect::<Vec<_>>(),
            vec!["push", "pull_request", "z_future", "workflow_call"]
        );

        let push = &rows[0];
        assert_eq!(push.entry_workflows, 2);
        assert_eq!(push.declarations, 2);
        assert_eq!(push.typed, 0);
        assert_eq!(push.filtered, 1);
        assert_eq!(
            push.examples,
            vec![
                ".github/workflows/a.yml".to_string(),
                ".github/workflows/b.yml".to_string()
            ]
        );

        let pull_request = rows.iter().find(|row| row.event == "pull_request").unwrap();
        assert_eq!(pull_request.entry_workflows, 1);
        assert_eq!(pull_request.declarations, 1);
        assert_eq!(pull_request.typed, 1);
        assert_eq!(pull_request.filtered, 1);

        let workflow_call = rows
            .iter()
            .find(|row| row.event == "workflow_call")
            .unwrap();
        assert_eq!(workflow_call.entry_workflows, 0);
        assert_eq!(workflow_call.declarations, 1);
    }

    #[test]
    fn examples_are_unique_sorted_and_capped() {
        let rows = triggers(&ir(vec![
            workflow(
                ".github/workflows/a.yml",
                vec![trigger(EventKind::Push), trigger(EventKind::Push)],
            ),
            workflow(".github/workflows/b.yml", vec![trigger(EventKind::Push)]),
            workflow(".github/workflows/c.yml", vec![trigger(EventKind::Push)]),
            workflow(".github/workflows/d.yml", vec![trigger(EventKind::Push)]),
        ]));

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].entry_workflows, 4);
        assert_eq!(rows[0].declarations, 5);
        assert_eq!(
            rows[0].examples,
            vec![
                ".github/workflows/a.yml".to_string(),
                ".github/workflows/b.yml".to_string(),
                ".github/workflows/c.yml".to_string()
            ]
        );
    }
}
