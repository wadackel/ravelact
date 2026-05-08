//! Shared outgoing-edge walker for query passes.
//!
//! Every query (`trace`, `orphans`, `impact`, `mermaid`) used to hand-roll
//! workflow / action traversal and decide independently whether annotations,
//! external workflows, docker refs, and local actions counted as edges. That
//! duplication produced inconsistencies (e.g. composite-annotation traversal
//! diverging across queries) and made future edge-kind additions expensive.
//!
//! [`for_each_outgoing_edge`] enumerates every outgoing edge of a [`Workflow`]
//! or [`LocalAction`] node exactly once, tagging each with the carrying
//! [`SourceTier`] and one of three edge kinds. Iteration order is fixed and
//! mirrors the historical order used by `trace`:
//!
//! 1. `node.annotations` (workflow-level / action-manifest-level)
//! 2. for each job (workflows only):
//!    - `job.annotations`
//!    - `job.calls_workflow`
//!    - for each step:
//!      - `step.annotations`
//!      - `step.uses`
//! 3. for each composite step (actions only):
//!    - `step.annotations`
//!    - `step.uses`
//!
//! Consumers re-derive any tier-specific behaviour (e.g. `if:` guard wrapping
//! in `trace`) from the [`SourceTier`] passed alongside the edge.

use crate::ir::*;
use core::ops::ControlFlow;

/// Top-level node whose outgoing edges are being enumerated.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Node<'a> {
    Workflow(&'a Workflow),
    Action(&'a LocalAction),
}

/// Where on the carrier the edge originates. Carries borrowed references so
/// consumers can read tier-specific fields (e.g. `Job.if_expr`). Variants
/// that no current consumer reads still carry their `&'a Step` so the source
/// tier is uniformly typed for future passes (e.g. propagating composite
/// step `if:` once the IR-level semantics are decided).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum SourceTier<'a> {
    /// Workflow-level (e.g. workflow-level annotation).
    Workflow,
    /// Job-level (e.g. job-level annotation, `job.calls_workflow`).
    Job(&'a Job),
    /// Step inside a workflow job (e.g. step annotation, step `uses:`).
    JobStep { job: &'a Job, step: &'a Step },
    /// Composite-action manifest level (e.g. composite manifest annotation).
    ActionManifest,
    /// Step inside a composite action (e.g. composite step annotation / `uses:`).
    ActionStep(&'a Step),
}

/// One outgoing edge from a [`Node`]. Variants are kept narrow so consumers
/// can match on the edge kind directly without re-classifying.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Edge<'a> {
    /// `# ravelact:` annotation. Both Resolved and Dangling are surfaced;
    /// consumers that only care about resolved targets must filter.
    Annotation(&'a Annotation),
    /// Job-level `uses:` (a reusable-workflow call).
    CallsWorkflow(&'a CallsWorkflow),
    /// Step-level `uses:` (local workflow / local action / external action / docker).
    Uses(&'a UsesRef),
}

/// One callback invocation: an [`Edge`] together with its [`SourceTier`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct EdgeContext<'a> {
    pub source: SourceTier<'a>,
    pub edge: Edge<'a>,
}

/// Visit every outgoing edge of `node` exactly once. See module docs for the
/// fixed iteration order. The visitor sees `Annotation` edges in resolved and
/// dangling forms alike — filtering is the consumer's responsibility.
///
/// The lifetime `'a` on both [`Node`] and [`EdgeContext`] ties each callback
/// argument to the node's borrow, so consumers can collect edges into
/// `Vec<(SourceTier<'a>, Edge<'a>)>` for deferred processing.
pub(crate) fn for_each_outgoing_edge<'a, F>(node: Node<'a>, mut visit: F)
where
    F: FnMut(EdgeContext<'a>),
{
    let _ = try_for_each_outgoing_edge::<_, ()>(node, |ctx| {
        visit(ctx);
        ControlFlow::Continue(())
    });
}

/// Short-circuiting variant of [`for_each_outgoing_edge`]. Iteration stops as
/// soon as the visitor returns [`ControlFlow::Break`], propagating the break
/// value to the caller. Iteration order matches [`for_each_outgoing_edge`].
pub(crate) fn try_for_each_outgoing_edge<'a, F, B>(node: Node<'a>, mut visit: F) -> ControlFlow<B>
where
    F: FnMut(EdgeContext<'a>) -> ControlFlow<B>,
{
    match node {
        Node::Workflow(wf) => visit_workflow(wf, &mut visit),
        Node::Action(act) => visit_action(act, &mut visit),
    }
}

fn visit_workflow<'a, F, B>(wf: &'a Workflow, visit: &mut F) -> ControlFlow<B>
where
    F: FnMut(EdgeContext<'a>) -> ControlFlow<B>,
{
    for ann in &wf.annotations {
        visit(EdgeContext {
            source: SourceTier::Workflow,
            edge: Edge::Annotation(ann),
        })?;
    }
    for job in &wf.jobs {
        for ann in &job.annotations {
            visit(EdgeContext {
                source: SourceTier::Job(job),
                edge: Edge::Annotation(ann),
            })?;
        }
        if let Some(call) = &job.calls_workflow {
            visit(EdgeContext {
                source: SourceTier::Job(job),
                edge: Edge::CallsWorkflow(call),
            })?;
        }
        for step in &job.steps {
            for ann in &step.annotations {
                visit(EdgeContext {
                    source: SourceTier::JobStep { job, step },
                    edge: Edge::Annotation(ann),
                })?;
            }
            if let Some(uses) = step.uses.as_ref() {
                visit(EdgeContext {
                    source: SourceTier::JobStep { job, step },
                    edge: Edge::Uses(uses),
                })?;
            }
        }
    }
    ControlFlow::Continue(())
}

fn visit_action<'a, F, B>(act: &'a LocalAction, visit: &mut F) -> ControlFlow<B>
where
    F: FnMut(EdgeContext<'a>) -> ControlFlow<B>,
{
    for ann in &act.annotations {
        visit(EdgeContext {
            source: SourceTier::ActionManifest,
            edge: Edge::Annotation(ann),
        })?;
    }
    for step in &act.steps {
        for ann in &step.annotations {
            visit(EdgeContext {
                source: SourceTier::ActionStep(step),
                edge: Edge::Annotation(ann),
            })?;
        }
        if let Some(uses) = step.uses.as_ref() {
            visit(EdgeContext {
                source: SourceTier::ActionStep(step),
                edge: Edge::Uses(uses),
            })?;
        }
    }
    ControlFlow::Continue(())
}

#[cfg(test)]
mod tests {
    //! Inline IR-builder helpers (`mk_step`, `mk_job`, `mk_workflow`,
    //! `mk_action`) construct `Workflow` / `Job` / `Step` / `LocalAction` via
    //! direct struct literals so each test pins one branch of
    //! `for_each_outgoing_edge` without going through the parser.
    use super::*;
    use std::path::PathBuf;

    fn empty_pos() -> SourcePos {
        SourcePos {
            file: PathBuf::new(),
            line: None,
        }
    }

    fn ann_resolved(verb: AnnotationVerb, target_id: &str) -> Annotation {
        Annotation {
            verb,
            resolution: AnnotationResolution::Resolved {
                target: WorkflowId(target_id.into()),
            },
            source_line: 1,
        }
    }

    fn mk_step(index: usize, uses: Option<UsesRef>, anns: Vec<Annotation>) -> Step {
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
            source: empty_pos(),
            annotations: anns,
        }
    }

    fn mk_job(
        wf_id: &str,
        id: &str,
        calls: Option<CallsWorkflow>,
        steps: Vec<Step>,
        anns: Vec<Annotation>,
    ) -> Job {
        Job {
            id: JobId(id.into()),
            workflow: WorkflowId(wf_id.into()),
            needs: vec![],
            permissions: None,
            steps,
            calls_workflow: calls,
            runs_on: None,
            outputs: Default::default(),
            source: empty_pos(),
            environment: None,
            if_expr: None,
            strategy: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            container: None,
            services: Default::default(),
            annotations: anns,
        }
    }

    fn mk_workflow(id: &str, jobs: Vec<Job>, anns: Vec<Annotation>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: empty_pos(),
            name: None,
            run_name: None,
            triggers: vec![],
            jobs,
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: anns,
        }
    }

    fn mk_action(id: &str, steps: Vec<Step>, anns: Vec<Annotation>) -> LocalAction {
        LocalAction {
            id: ActionId(id.into()),
            source: empty_pos(),
            name: None,
            kind: ActionKind::Composite,
            inputs: vec![],
            outputs: vec![],
            steps,
            annotations: anns,
        }
    }

    /// Tag of one observed edge, sufficient for ordering / classification
    /// assertions without re-deriving life-times in test data.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum EdgeTag {
        WorkflowAnn,
        JobAnn(String),
        JobCall(String),
        JobStepAnn(String, usize),
        JobStepUses(String, usize),
        ActionManifestAnn,
        ActionStepAnn(usize),
        ActionStepUses(usize),
    }

    fn collect_tags_workflow(wf: &Workflow) -> Vec<EdgeTag> {
        let mut buf: Vec<EdgeTag> = Vec::new();
        for_each_outgoing_edge(Node::Workflow(wf), |ctx| {
            let tag = match (ctx.source, ctx.edge) {
                (SourceTier::Workflow, Edge::Annotation(_)) => EdgeTag::WorkflowAnn,
                (SourceTier::Job(j), Edge::Annotation(_)) => EdgeTag::JobAnn(j.id.0.clone()),
                (SourceTier::Job(j), Edge::CallsWorkflow(_)) => EdgeTag::JobCall(j.id.0.clone()),
                (SourceTier::JobStep { job, step }, Edge::Annotation(_)) => {
                    EdgeTag::JobStepAnn(job.id.0.clone(), step.index)
                }
                (SourceTier::JobStep { job, step }, Edge::Uses(_)) => {
                    EdgeTag::JobStepUses(job.id.0.clone(), step.index)
                }
                other => panic!("unexpected workflow edge: {other:?}"),
            };
            buf.push(tag);
        });
        buf
    }

    fn collect_tags_action(act: &LocalAction) -> Vec<EdgeTag> {
        let mut buf: Vec<EdgeTag> = Vec::new();
        for_each_outgoing_edge(Node::Action(act), |ctx| {
            let tag = match (ctx.source, ctx.edge) {
                (SourceTier::ActionManifest, Edge::Annotation(_)) => EdgeTag::ActionManifestAnn,
                (SourceTier::ActionStep(step), Edge::Annotation(_)) => {
                    EdgeTag::ActionStepAnn(step.index)
                }
                (SourceTier::ActionStep(step), Edge::Uses(_)) => {
                    EdgeTag::ActionStepUses(step.index)
                }
                other => panic!("unexpected action edge: {other:?}"),
            };
            buf.push(tag);
        });
        buf
    }

    #[test]
    fn workflow_with_no_outgoing_edges_emits_nothing() {
        let wf = mk_workflow(".github/workflows/empty.yml", vec![], vec![]);
        assert!(collect_tags_workflow(&wf).is_empty());
    }

    #[test]
    fn workflow_level_annotation_emits_workflow_tier() {
        let wf = mk_workflow(
            ".github/workflows/wf.yml",
            vec![],
            vec![ann_resolved(
                AnnotationVerb::Dispatches,
                ".github/workflows/build.yml",
            )],
        );
        assert_eq!(collect_tags_workflow(&wf), vec![EdgeTag::WorkflowAnn]);
    }

    #[test]
    fn job_level_annotation_and_calls_workflow_emit_job_tier() {
        let job = mk_job(
            ".github/workflows/wf.yml",
            "deploy",
            Some(CallsWorkflow {
                workflow_ref: WorkflowRef::Local(WorkflowId(
                    ".github/workflows/reusable.yml".into(),
                )),
                with: Default::default(),
                secrets: SecretsPass::None,
            }),
            vec![],
            vec![ann_resolved(
                AnnotationVerb::Triggers,
                ".github/workflows/audit.yml",
            )],
        );
        let wf = mk_workflow(".github/workflows/wf.yml", vec![job], vec![]);
        assert_eq!(
            collect_tags_workflow(&wf),
            vec![
                EdgeTag::JobAnn("deploy".into()),
                EdgeTag::JobCall("deploy".into()),
            ]
        );
    }

    #[test]
    fn step_annotation_and_uses_emit_jobstep_tier() {
        let step = mk_step(
            3,
            Some(UsesRef::LocalAction(ActionId(".github/actions/x".into()))),
            vec![ann_resolved(
                AnnotationVerb::Dispatches,
                ".github/workflows/notify.yml",
            )],
        );
        let job = mk_job(
            ".github/workflows/wf.yml",
            "build",
            None,
            vec![step],
            vec![],
        );
        let wf = mk_workflow(".github/workflows/wf.yml", vec![job], vec![]);
        assert_eq!(
            collect_tags_workflow(&wf),
            vec![
                EdgeTag::JobStepAnn("build".into(), 3),
                EdgeTag::JobStepUses("build".into(), 3),
            ]
        );
    }

    #[test]
    fn iteration_order_is_workflow_then_job_then_step() {
        // Verify the documented fixed order is honoured: WF-ann, JOB-ann,
        // JOB-call, JOB-step-ann, JOB-step-uses.
        let step = mk_step(
            0,
            Some(UsesRef::LocalAction(ActionId(".github/actions/x".into()))),
            vec![ann_resolved(
                AnnotationVerb::Dispatches,
                ".github/workflows/a.yml",
            )],
        );
        let job = mk_job(
            ".github/workflows/wf.yml",
            "j",
            Some(CallsWorkflow {
                workflow_ref: WorkflowRef::Local(WorkflowId(".github/workflows/r.yml".into())),
                with: Default::default(),
                secrets: SecretsPass::None,
            }),
            vec![step],
            vec![ann_resolved(
                AnnotationVerb::Triggers,
                ".github/workflows/b.yml",
            )],
        );
        let wf = mk_workflow(
            ".github/workflows/wf.yml",
            vec![job],
            vec![ann_resolved(
                AnnotationVerb::Dispatches,
                ".github/workflows/c.yml",
            )],
        );
        assert_eq!(
            collect_tags_workflow(&wf),
            vec![
                EdgeTag::WorkflowAnn,
                EdgeTag::JobAnn("j".into()),
                EdgeTag::JobCall("j".into()),
                EdgeTag::JobStepAnn("j".into(), 0),
                EdgeTag::JobStepUses("j".into(), 0),
            ]
        );
    }

    #[test]
    fn action_manifest_and_step_emit_action_tiers() {
        let step0 = mk_step(
            0,
            None,
            vec![ann_resolved(
                AnnotationVerb::Dispatches,
                ".github/workflows/x.yml",
            )],
        );
        let step1 = mk_step(
            1,
            Some(UsesRef::LocalAction(ActionId(".github/actions/y".into()))),
            vec![],
        );
        let act = mk_action(
            ".github/actions/composite",
            vec![step0, step1],
            vec![ann_resolved(
                AnnotationVerb::Triggers,
                ".github/workflows/audit.yml",
            )],
        );
        assert_eq!(
            collect_tags_action(&act),
            vec![
                EdgeTag::ActionManifestAnn,
                EdgeTag::ActionStepAnn(0),
                EdgeTag::ActionStepUses(1),
            ]
        );
    }

    #[test]
    fn try_for_each_outgoing_edge_breaks_on_first_match() {
        // Two annotations and one step `uses:`. The visitor breaks on the
        // second visit (the second annotation), so the third edge (the step
        // uses) must never be observed.
        let step = mk_step(
            0,
            Some(UsesRef::LocalAction(ActionId(".github/actions/x".into()))),
            vec![],
        );
        let job = mk_job(
            ".github/workflows/wf.yml",
            "j",
            None,
            vec![step],
            vec![ann_resolved(
                AnnotationVerb::Triggers,
                ".github/workflows/b.yml",
            )],
        );
        let wf = mk_workflow(
            ".github/workflows/wf.yml",
            vec![job],
            vec![ann_resolved(
                AnnotationVerb::Dispatches,
                ".github/workflows/a.yml",
            )],
        );

        let mut count = 0usize;
        let result = try_for_each_outgoing_edge::<_, &'static str>(Node::Workflow(&wf), |_ctx| {
            count += 1;
            if count == 2 {
                ControlFlow::Break("stop")
            } else {
                ControlFlow::Continue(())
            }
        });
        assert_eq!(result, ControlFlow::Break("stop"));
        assert_eq!(count, 2, "iteration must short-circuit on Break");
    }

    #[test]
    fn try_for_each_outgoing_edge_continue_visits_every_edge() {
        // Continue-only run must visit every outgoing edge and finish with
        // ControlFlow::Continue(()).
        let step = mk_step(
            0,
            Some(UsesRef::LocalAction(ActionId(".github/actions/x".into()))),
            vec![],
        );
        let job = mk_job(".github/workflows/wf.yml", "j", None, vec![step], vec![]);
        let wf = mk_workflow(
            ".github/workflows/wf.yml",
            vec![job],
            vec![ann_resolved(
                AnnotationVerb::Dispatches,
                ".github/workflows/a.yml",
            )],
        );

        let mut count = 0usize;
        let result = try_for_each_outgoing_edge::<_, ()>(Node::Workflow(&wf), |_ctx| {
            count += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(result, ControlFlow::Continue(()));
        assert_eq!(count, 2, "Continue-only must visit every outgoing edge");
    }

    #[test]
    fn dangling_annotations_are_visited_too() {
        // The visitor surfaces both Resolved and Dangling annotations; filtering
        // is the consumer's responsibility per the module docstring.
        let ann = Annotation {
            verb: AnnotationVerb::Dispatches,
            resolution: AnnotationResolution::Dangling {
                raw_target: "missing".into(),
                reason: "not found".into(),
            },
            source_line: 1,
        };
        let wf = mk_workflow(".github/workflows/wf.yml", vec![], vec![ann]);
        assert_eq!(collect_tags_workflow(&wf), vec![EdgeTag::WorkflowAnn]);
    }
}
