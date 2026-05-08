use crate::ir::*;
use crate::query::walk::{try_for_each_outgoing_edge, Edge, Node};
use core::ops::ControlFlow;
use std::collections::{BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpactResult {
    /// Entry-point workflows transitively reaching any of the changed nodes.
    /// Reusable workflows (`workflow_call`-only) are not listed; they only
    /// serve as path edges in the reverse traversal. Workflow seed inputs
    /// themselves are excluded — only downstream callers are reported.
    pub workflows: Vec<WorkflowId>,
    /// Local actions that transitively consume a changed local action. Each
    /// entry carries the action's `kind` so the CLI can render per-kind labels
    /// without re-reading the IR. Seed inputs themselves are excluded — only
    /// downstream consumers are reported.
    pub actions: Vec<(ActionId, ActionKind)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputClassification {
    Workflow(WorkflowId),
    Action(ActionId),
    Unknown(String),
}

/// Resolve a single input path to an IR node.
pub fn classify_input(ir: &Ir, path: &str) -> InputClassification {
    let normalized = path
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string();

    if (normalized.ends_with(".yml") || normalized.ends_with(".yaml"))
        && normalized.starts_with(".github/workflows/")
    {
        let id = WorkflowId(normalized.clone());
        if ir.workflows.iter().any(|w| w.id == id) {
            return InputClassification::Workflow(id);
        }
    }

    let basename = normalized.rsplit('/').next().unwrap_or("");
    if basename == "action.yml" || basename == "action.yaml" {
        if let Some(parent) = normalized.strip_suffix(&format!("/{basename}")) {
            let id = ActionId(parent.to_string());
            if ir.actions.iter().any(|c| c.id == id) {
                return InputClassification::Action(id);
            }
        }
    }

    let mut candidates: Vec<&LocalAction> = ir
        .actions
        .iter()
        .filter(|c| {
            let cid = &c.id.0;
            normalized == *cid || normalized.starts_with(&format!("{cid}/"))
        })
        .collect();
    candidates.sort_by_key(|c| std::cmp::Reverse(c.id.0.len()));
    if let Some(best) = candidates.first() {
        return InputClassification::Action(best.id.clone());
    }

    InputClassification::Unknown(path.to_string())
}

pub fn impact(ir: &Ir, files: &[String]) -> (ImpactResult, Vec<String>) {
    let mut seed_wfs: BTreeSet<String> = BTreeSet::new();
    let mut seed_actions: BTreeSet<String> = BTreeSet::new();
    let mut unknowns: Vec<String> = Vec::new();

    for f in files {
        match classify_input(ir, f) {
            InputClassification::Workflow(WorkflowId(p)) => {
                seed_wfs.insert(p);
            }
            InputClassification::Action(ActionId(p)) => {
                seed_actions.insert(p);
            }
            InputClassification::Unknown(orig) => unknowns.push(orig),
        }
    }

    let (visited_wf, visited_act) = reverse_bfs(ir, &seed_wfs, &seed_actions);

    // Seeds were inserted into `visited_*` to prevent re-visit during BFS;
    // exclude them here so the user-facing result lists only downstream
    // consumers, not the inputs themselves.
    let mut workflows: Vec<WorkflowId> = visited_wf
        .iter()
        .filter(|p| !seed_wfs.contains(*p))
        .filter_map(|p| ir.workflows.iter().find(|w| &w.id.0 == p))
        .filter(|w| w.triggers.iter().any(|t| t.is_entry_point()))
        .map(|w| w.id.clone())
        .collect();
    workflows.sort_by(|a, b| a.0.cmp(&b.0));

    let mut actions: Vec<(ActionId, ActionKind)> = visited_act
        .iter()
        .filter(|p| !seed_actions.contains(*p))
        .filter_map(|p| {
            ir.actions
                .iter()
                .find(|a| &a.id.0 == p)
                .map(|a| (a.id.clone(), a.kind.clone()))
        })
        .collect();
    actions.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));

    (ImpactResult { workflows, actions }, unknowns)
}

#[derive(Debug)]
enum Seed {
    Wf(String),
    Act(String),
}

fn reverse_bfs(
    ir: &Ir,
    seed_wfs: &BTreeSet<String>,
    seed_actions: &BTreeSet<String>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut visited_wf: BTreeSet<String> = seed_wfs.clone();
    let mut visited_act: BTreeSet<String> = seed_actions.clone();
    let mut queue: VecDeque<Seed> = VecDeque::new();
    for w in seed_wfs {
        queue.push_back(Seed::Wf(w.clone()));
    }
    for a in seed_actions {
        queue.push_back(Seed::Act(a.clone()));
    }

    while let Some(seed) = queue.pop_front() {
        match seed {
            Seed::Wf(target) => {
                for wf in &ir.workflows {
                    if calls_workflow(wf, &target) && visited_wf.insert(wf.id.0.clone()) {
                        queue.push_back(Seed::Wf(wf.id.0.clone()));
                    }
                }
                // Composite actions can dispatch / trigger workflows via
                // `# ravelact:` annotations. A composite that targets the changed
                // workflow is impacted; its `Seed::Act` arm then propagates the
                // impact to every workflow that uses the composite.
                for comp in &ir.actions {
                    if composite_targets_workflow(comp, &target)
                        && visited_act.insert(comp.id.0.clone())
                    {
                        queue.push_back(Seed::Act(comp.id.0.clone()));
                    }
                }
            }
            Seed::Act(target) => {
                for wf in &ir.workflows {
                    if uses_action_in_workflow(wf, &target) && visited_wf.insert(wf.id.0.clone()) {
                        queue.push_back(Seed::Wf(wf.id.0.clone()));
                    }
                }
                for comp in &ir.actions {
                    if uses_action_in_composite(comp, &target)
                        && visited_act.insert(comp.id.0.clone())
                    {
                        queue.push_back(Seed::Act(comp.id.0.clone()));
                    }
                }
            }
        }
    }

    (visited_wf, visited_act)
}

/// Returns true when any outgoing edge of `wf` reaches the workflow id
/// `target` — either as a Resolved annotation, a `calls_workflow` Local ref,
/// or a step `uses: LocalWorkflow`.
fn calls_workflow(wf: &Workflow, target: &str) -> bool {
    try_for_each_outgoing_edge(Node::Workflow(wf), |ctx| {
        if workflow_edge_targets_workflow(&ctx.edge, target) {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .is_break()
}

/// Returns true when any outgoing `Edge::Uses` of `wf` reaches the local
/// action `target`. Annotation and `calls_workflow` edges never carry a
/// local-action target so they are skipped.
fn uses_action_in_workflow(wf: &Workflow, target: &str) -> bool {
    try_for_each_outgoing_edge(Node::Workflow(wf), |ctx| {
        if let Edge::Uses(UsesRef::LocalAction(ActionId(p))) = ctx.edge {
            if p == target {
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    })
    .is_break()
}

/// Returns true when any composite-step `uses:` reaches the local action
/// `target`.
fn uses_action_in_composite(comp: &LocalAction, target: &str) -> bool {
    try_for_each_outgoing_edge(Node::Action(comp), |ctx| {
        if let Edge::Uses(UsesRef::LocalAction(ActionId(p))) = ctx.edge {
            if p == target {
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    })
    .is_break()
}

/// Returns true when the composite action carries a Resolved annotation —
/// either at the manifest level or on one of its `runs.steps` — pointing at
/// `target` (a workflow id). Composite `Edge::Uses` edges never target a
/// workflow id (only `Edge::CallsWorkflow` does, and composites cannot call
/// reusable workflows), so this only inspects annotations.
fn composite_targets_workflow(comp: &LocalAction, target: &str) -> bool {
    try_for_each_outgoing_edge(Node::Action(comp), |ctx| {
        if let Edge::Annotation(ann) = ctx.edge {
            if let AnnotationResolution::Resolved { target: t } = &ann.resolution {
                if t.0 == target {
                    return ControlFlow::Break(());
                }
            }
        }
        ControlFlow::Continue(())
    })
    .is_break()
}

fn workflow_edge_targets_workflow(edge: &Edge<'_>, target: &str) -> bool {
    match edge {
        Edge::Annotation(ann) => matches!(
            &ann.resolution,
            AnnotationResolution::Resolved { target: t } if t.0 == target
        ),
        Edge::CallsWorkflow(call) => matches!(
            &call.workflow_ref,
            WorkflowRef::Local(WorkflowId(p)) if p == target
        ),
        Edge::Uses(UsesRef::LocalWorkflow(WorkflowId(p))) => p == target,
        Edge::Uses(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn empty_step(index: usize, uses: Option<UsesRef>) -> Step {
        Step {
            index,
            id: None,
            name: None,
            uses,
            run: None,
            if_expr: None,
            with: Default::default(),
            env: Default::default(),
            shell: None,
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            annotations: Vec::new(),
        }
    }

    fn workflow(id: &str, triggers: Vec<TriggerSpec>, jobs: Vec<Job>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            run_name: None,
            triggers,
            jobs,
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: Vec::new(),
        }
    }

    fn job(workflow_id: &str, id: &str, steps: Vec<Step>, calls: Option<CallsWorkflow>) -> Job {
        Job {
            id: JobId(id.into()),
            workflow: WorkflowId(workflow_id.into()),
            needs: vec![],
            permissions: None,
            steps,
            calls_workflow: calls,
            runs_on: None,
            outputs: Default::default(),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            environment: None,
            if_expr: None,
            strategy: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            container: None,
            services: Default::default(),
            annotations: Vec::new(),
        }
    }

    fn composite(id: &str, steps: Vec<Step>) -> LocalAction {
        LocalAction {
            id: ActionId(id.into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            kind: ActionKind::Composite,
            inputs: vec![],
            outputs: vec![],
            steps,
            annotations: Vec::new(),
        }
    }

    fn ir_with(workflows: Vec<Workflow>, actions: Vec<LocalAction>) -> Ir {
        Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows,
            actions,
            external_actions: vec![],
        }
    }

    fn push_trigger() -> TriggerSpec {
        TriggerSpec::bare(EventKind::Push)
    }

    fn workflow_call_trigger() -> TriggerSpec {
        TriggerSpec::bare(EventKind::WorkflowCall)
    }

    #[test]
    fn impact_resolves_workflow_input_to_entry_point_callers() {
        let ci = workflow(
            ".github/workflows/ci.yml",
            vec![push_trigger()],
            vec![job(
                ".github/workflows/ci.yml",
                "call-build",
                vec![],
                Some(CallsWorkflow {
                    workflow_ref: WorkflowRef::Local(WorkflowId(
                        ".github/workflows/build.yml".into(),
                    )),
                    with: Default::default(),
                    secrets: SecretsPass::Inherit,
                }),
            )],
        );
        let build = workflow(
            ".github/workflows/build.yml",
            vec![workflow_call_trigger()],
            vec![],
        );
        let ir = ir_with(vec![ci, build], vec![]);

        let (result, unknowns) = impact(&ir, &[".github/workflows/build.yml".into()]);
        assert!(unknowns.is_empty());
        assert_eq!(
            result.workflows,
            vec![WorkflowId(".github/workflows/ci.yml".into())]
        );
        assert!(result.actions.is_empty());
    }

    #[test]
    fn impact_resolves_action_input_to_workflow_and_composite_consumers() {
        let setup = composite(".github/actions/setup", vec![]);
        let inner = composite(".github/actions/inner", vec![]);
        let outer = composite(
            ".github/actions/outer",
            vec![empty_step(
                0,
                Some(UsesRef::LocalAction(ActionId(
                    ".github/actions/inner".into(),
                ))),
            )],
        );
        let ci = workflow(
            ".github/workflows/ci.yml",
            vec![push_trigger()],
            vec![job(
                ".github/workflows/ci.yml",
                "test",
                vec![
                    empty_step(
                        0,
                        Some(UsesRef::LocalAction(ActionId(
                            ".github/actions/setup".into(),
                        ))),
                    ),
                    empty_step(
                        1,
                        Some(UsesRef::LocalAction(ActionId(
                            ".github/actions/outer".into(),
                        ))),
                    ),
                ],
                None,
            )],
        );
        let ir = ir_with(vec![ci], vec![setup, inner, outer]);

        let (result, _) = impact(&ir, &[".github/actions/setup".into()]);
        assert_eq!(
            result.workflows,
            vec![WorkflowId(".github/workflows/ci.yml".into())]
        );
        assert!(
            result.actions.is_empty(),
            "seed composite must be excluded from result, got {:?}",
            result.actions
        );
    }

    #[test]
    fn impact_filters_reusable_only_workflows() {
        let reusable = workflow(
            ".github/workflows/reusable.yml",
            vec![workflow_call_trigger()],
            vec![job(
                ".github/workflows/reusable.yml",
                "use",
                vec![empty_step(
                    0,
                    Some(UsesRef::LocalAction(ActionId(
                        ".github/actions/setup".into(),
                    ))),
                )],
                None,
            )],
        );
        let setup = composite(".github/actions/setup", vec![]);
        let ir = ir_with(vec![reusable], vec![setup]);

        let (result, _) = impact(&ir, &[".github/actions/setup".into()]);
        // reusable.yml is the only caller but it's workflow_call-only → must be excluded.
        assert!(
            result.workflows.is_empty(),
            "reusable-only workflow must not appear in entry-point list, got {:?}",
            result.workflows
        );
        // seed composite must also be excluded from the result.
        assert!(
            result.actions.is_empty(),
            "seed composite must be excluded from result, got {:?}",
            result.actions
        );
    }

    #[test]
    fn impact_unknown_path_returns_warning_classification() {
        let ir = ir_with(vec![], vec![]);
        let cls = classify_input(&ir, "scripts/foo.sh");
        assert!(matches!(cls, InputClassification::Unknown(ref s) if s == "scripts/foo.sh"));

        let (result, unknowns) = impact(&ir, &["scripts/foo.sh".into()]);
        assert_eq!(unknowns, vec!["scripts/foo.sh".to_string()]);
        assert!(result.workflows.is_empty());
        assert!(result.actions.is_empty());
    }

    #[test]
    fn impact_resolves_composite_subpath_input() {
        let setup = composite(".github/actions/setup", vec![]);
        let nested = composite(".github/actions/setup/nested", vec![]);
        let ir = ir_with(vec![], vec![setup, nested]);

        // direct action.yml path
        let cls1 = classify_input(&ir, ".github/actions/setup/action.yml");
        assert_eq!(
            cls1,
            InputClassification::Action(ActionId(".github/actions/setup".into()))
        );

        // file under composite directory → resolves to most-specific composite (nested wins)
        let cls2 = classify_input(&ir, ".github/actions/setup/nested/scripts/foo.sh");
        assert_eq!(
            cls2,
            InputClassification::Action(ActionId(".github/actions/setup/nested".into()))
        );

        // ./ prefix is stripped (parse_uses invariant)
        let cls3 = classify_input(&ir, "./.github/actions/setup/scripts/foo.sh");
        assert_eq!(
            cls3,
            InputClassification::Action(ActionId(".github/actions/setup".into()))
        );

        // trailing slash is tolerated
        let cls4 = classify_input(&ir, ".github/actions/setup/");
        assert_eq!(
            cls4,
            InputClassification::Action(ActionId(".github/actions/setup".into()))
        );
    }

    #[test]
    fn impact_resolves_annotated_dispatch_to_caller() {
        // trigger.yml has a step-level `# ravelact:dispatches build.yml`
        // annotation; build.yml is workflow_call-only. `impact build.yml`
        // must surface trigger.yml as an entry-point caller via the
        // annotation, not via any structural `uses:`.
        let mut trigger = workflow(
            ".github/workflows/trigger.yml",
            vec![push_trigger()],
            vec![],
        );
        // Synthesize a step with an annotation pointing at build.yml.
        let mut job_t = job(
            ".github/workflows/trigger.yml",
            "run",
            vec![empty_step(0, None)],
            None,
        );
        job_t.steps[0].annotations.push(Annotation {
            verb: AnnotationVerb::Dispatches,
            resolution: AnnotationResolution::Resolved {
                target: WorkflowId(".github/workflows/build.yml".into()),
            },
            source_line: 6,
        });
        trigger.jobs.push(job_t);

        let build = workflow(
            ".github/workflows/build.yml",
            vec![workflow_call_trigger()],
            vec![],
        );
        let ir = ir_with(vec![trigger, build], vec![]);

        let (result, unknowns) = impact(&ir, &[".github/workflows/build.yml".into()]);
        assert!(unknowns.is_empty());
        assert_eq!(
            result.workflows,
            vec![WorkflowId(".github/workflows/trigger.yml".into())],
            "annotation-only caller must appear as entry-point"
        );
    }

    #[test]
    fn impact_transitive_composite_consumer_chain() {
        let action_b = composite(".github/actions/b", vec![]);
        let action_a = composite(
            ".github/actions/a",
            vec![empty_step(
                0,
                Some(UsesRef::LocalAction(ActionId(".github/actions/b".into()))),
            )],
        );
        let main_wf = workflow(
            ".github/workflows/main.yml",
            vec![push_trigger()],
            vec![job(
                ".github/workflows/main.yml",
                "run",
                vec![empty_step(
                    0,
                    Some(UsesRef::LocalAction(ActionId(".github/actions/a".into()))),
                )],
                None,
            )],
        );
        let ir = ir_with(vec![main_wf], vec![action_a, action_b]);

        let (result, _) = impact(&ir, &[".github/actions/b".into()]);
        assert_eq!(
            result.workflows,
            vec![WorkflowId(".github/workflows/main.yml".into())],
            "entry-point workflow reaching action-a must be listed"
        );
        assert_eq!(
            result.actions,
            vec![(ActionId(".github/actions/a".into()), ActionKind::Composite,)],
            "consumer (a) must be in actions; seed (b) must be excluded"
        );
    }

    #[test]
    fn impact_excludes_entry_point_seed_from_workflows() {
        // shared.yml has both push and workflow_call triggers (a "both" workflow).
        // It is reachable in `visited_wf` as the seed and it is itself an
        // entry-point. The new filter must exclude the seed from the final
        // workflows output — the contract is "downstream callers only",
        // symmetric to composite seed exclusion.
        let shared = workflow(
            ".github/workflows/shared.yml",
            vec![push_trigger(), workflow_call_trigger()],
            vec![job(".github/workflows/shared.yml", "run", vec![], None)],
        );
        let release = workflow(
            ".github/workflows/release.yml",
            vec![push_trigger()],
            vec![job(
                ".github/workflows/release.yml",
                "deploy",
                vec![],
                Some(CallsWorkflow {
                    workflow_ref: WorkflowRef::Local(WorkflowId(
                        ".github/workflows/shared.yml".into(),
                    )),
                    with: Default::default(),
                    secrets: SecretsPass::Inherit,
                }),
            )],
        );
        let ir = ir_with(vec![shared, release], vec![]);

        let (result, _) = impact(&ir, &[".github/workflows/shared.yml".into()]);
        assert_eq!(
            result.workflows,
            vec![WorkflowId(".github/workflows/release.yml".into())],
            "entry-point seed (shared.yml) must be excluded; only downstream caller (release.yml) remains, got {:?}",
            result.workflows
        );
        assert!(result.actions.is_empty());
    }
}
