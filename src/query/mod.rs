pub mod callers;
pub mod dedup;
pub mod impact;
pub mod mermaid;
pub mod orphans;
pub mod trace;
pub mod trace_render;
pub mod triggers;
pub(crate) mod walk;
pub mod workflow_run;

use crate::ir::*;
use crate::parser::annotations::line_starts_with_ravelact;
use crate::query::workflow_run as wr;

/// Resolution target for `callers` lookup. The user passes a path
/// (relative to root) which can be either a workflow file or an action
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Workflow(WorkflowId),
    Action(ActionId),
}

impl Target {
    /// Heuristic: anything ending with `.yml`/`.yaml` under `.github/workflows/`
    /// is a workflow; everything else is an action directory.
    pub fn from_user_input(s: &str) -> Self {
        let normalized = s.trim_start_matches("./").trim_end_matches('/').to_string();
        let is_yaml = normalized.ends_with(".yml") || normalized.ends_with(".yaml");
        if is_yaml && normalized.starts_with(".github/workflows/") {
            Target::Workflow(WorkflowId(normalized))
        } else {
            Target::Action(ActionId(normalized))
        }
    }
}

/// Run the `wiring` pass against the IR. Four finding kinds are produced:
///
/// 1. **UnannotatedDispatch** — a `gh workflow run X` literal in a step's
///    `run:` body that is not paired with a matching `# ravelact:dispatches X`
///    annotation on the same step.
/// 2. **DanglingAnnotation** — an `# ravelact:` comment whose `<ref>` could not
///    be resolved to a local workflow.
/// 3. **DanglingWorkflowRun** — a `workflow_run.workflows: [Name]` entry that
///    could not be resolved to any local workflow by display name or path.
/// 4. **DanglingLocalUses** — a `uses: ./<path>` reference (step-level local
///    action or job-level local reusable workflow) whose target is not
///    present in the IR. Catches typos and stale paths that would otherwise
///    silently disappear from `graph` and panic prior to this fix.
///
/// The scanner is line-local and only matches literal targets. Variable
/// expansion (`gh workflow run "$VAR"`), shell line continuation (`\`), and
/// command substitution (`$(...)`) are out of scope and documented as known
/// false negatives in the README.
//
// `collapsible_match` would collapse `Some(UsesRef::X(t)) => { if guard { ... } }`
// arms into guarded patterns, but doing so would force a `_ => {}` catch-all and
// lose exhaustiveness checking against future `UsesRef` variants. Same rationale
// as `mermaid::render`.
#[allow(clippy::collapsible_match)]
pub fn wiring(ir: &Ir) -> Vec<WiringFinding> {
    let mut findings: Vec<WiringFinding> = Vec::new();

    // Build O(1) lookups for local target existence checks.
    let wf_ids: std::collections::HashSet<&str> =
        ir.workflows.iter().map(|w| w.id.0.as_str()).collect();
    let act_ids: std::collections::HashSet<&str> =
        ir.actions.iter().map(|a| a.id.0.as_str()).collect();

    // Build the workflow_run name index once so dangling-name detection is O(n).
    let wr_index = wr::build_index(ir);
    let (_resolved, dangling_wr) = wr::partition_workflow_run_names(ir, &wr_index);
    for (declaring_id, raw_name) in dangling_wr {
        // Locate the source file for the declaring workflow.
        if let Some(declaring_wf) = ir.workflows.iter().find(|w| &w.id == declaring_id) {
            findings.push(WiringFinding {
                file: declaring_wf.source.file.clone(),
                // Best-effort line: use the workflow source line; trigger-level
                // line numbers are not yet stored in the IR.
                line: declaring_wf.source.line.unwrap_or(0),
                kind: WiringKind::DanglingWorkflowRun {
                    raw_name: raw_name.to_string(),
                },
            });
        }
    }

    // Dangling annotations carried by composite action manifests / steps.
    for action in &ir.actions {
        let file = action.source.file.clone();
        push_dangling(&file, &action.annotations, &mut findings);
        for step in &action.steps {
            push_dangling(&file, &step.annotations, &mut findings);
        }
    }

    for wf in &ir.workflows {
        let file = wf.source.file.clone();

        // Dangling annotation findings — workflow / job / step tier.
        push_dangling(&file, &wf.annotations, &mut findings);
        for job in &wf.jobs {
            push_dangling(&file, &job.annotations, &mut findings);

            // Job-level `uses: ./<workflow>.yml` (reusable workflow call).
            if let Some(call) = &job.calls_workflow {
                if let WorkflowRef::Local(target) = &call.workflow_ref {
                    if !wf_ids.contains(target.0.as_str()) {
                        findings.push(WiringFinding {
                            file: file.clone(),
                            line: job.source.line.unwrap_or(0),
                            kind: WiringKind::DanglingLocalUses {
                                local_kind: DanglingLocalUsesKind::Workflow,
                                raw_target: target.0.clone(),
                            },
                        });
                    }
                }
            }

            for step in &job.steps {
                push_dangling(&file, &step.annotations, &mut findings);

                // Step-level `uses: ./<path>` (local action or local workflow).
                match step.uses.as_ref() {
                    Some(UsesRef::LocalAction(target)) => {
                        if !act_ids.contains(target.0.as_str()) {
                            findings.push(WiringFinding {
                                file: file.clone(),
                                line: step.source.line.unwrap_or(0),
                                kind: WiringKind::DanglingLocalUses {
                                    local_kind: DanglingLocalUsesKind::Action,
                                    raw_target: target.0.clone(),
                                },
                            });
                        }
                    }
                    Some(UsesRef::LocalWorkflow(target)) => {
                        if !wf_ids.contains(target.0.as_str()) {
                            findings.push(WiringFinding {
                                file: file.clone(),
                                line: step.source.line.unwrap_or(0),
                                kind: WiringKind::DanglingLocalUses {
                                    local_kind: DanglingLocalUsesKind::Workflow,
                                    raw_target: target.0.clone(),
                                },
                            });
                        }
                    }
                    _ => {}
                }

                // Unannotated `gh workflow run X` findings.
                let Some(body) = &step.run else {
                    continue;
                };
                let step_line = step.source.line.unwrap_or(0);
                for (offset, line) in body.lines().enumerate() {
                    let line_no = step_line.saturating_add(offset);
                    let trimmed = line.trim_start();
                    // Skip comment lines (any # comment, including ravelact).
                    if trimmed.starts_with('#') {
                        continue;
                    }
                    if line_starts_with_ravelact(line) {
                        continue;
                    }
                    let Some(target) = extract_gh_workflow_run(line) else {
                        continue;
                    };
                    if step_has_dispatch_annotation(&step.annotations, target) {
                        continue;
                    }
                    findings.push(WiringFinding {
                        file: file.clone(),
                        line: line_no,
                        kind: WiringKind::UnannotatedDispatch {
                            raw_target: target.to_string(),
                        },
                    });
                }
            }
        }
    }

    // Composite action steps can also reference local actions / workflows.
    for action in &ir.actions {
        for step in &action.steps {
            match step.uses.as_ref() {
                Some(UsesRef::LocalAction(target)) => {
                    if !act_ids.contains(target.0.as_str()) {
                        findings.push(WiringFinding {
                            file: action.source.file.clone(),
                            line: step.source.line.unwrap_or(0),
                            kind: WiringKind::DanglingLocalUses {
                                local_kind: DanglingLocalUsesKind::Action,
                                raw_target: target.0.clone(),
                            },
                        });
                    }
                }
                Some(UsesRef::LocalWorkflow(target)) => {
                    if !wf_ids.contains(target.0.as_str()) {
                        findings.push(WiringFinding {
                            file: action.source.file.clone(),
                            line: step.source.line.unwrap_or(0),
                            kind: WiringKind::DanglingLocalUses {
                                local_kind: DanglingLocalUsesKind::Workflow,
                                raw_target: target.0.clone(),
                            },
                        });
                    }
                }
                _ => {}
            }
        }
    }

    findings
}

fn push_dangling(file: &std::path::Path, anns: &[Annotation], out: &mut Vec<WiringFinding>) {
    for a in anns {
        if let AnnotationResolution::Dangling { raw_target, reason } = &a.resolution {
            out.push(WiringFinding {
                file: file.to_path_buf(),
                line: a.source_line,
                kind: WiringKind::DanglingAnnotation {
                    raw_target: raw_target.clone(),
                    reason: reason.clone(),
                },
            });
        }
    }
}

fn step_has_dispatch_annotation(anns: &[Annotation], target: &str) -> bool {
    let want = workflow_basename(target);
    anns.iter().any(|a| match (&a.verb, &a.resolution) {
        (AnnotationVerb::Dispatches, AnnotationResolution::Resolved { target: t }) => {
            workflow_basename(&t.0) == want
        }
        _ => false,
    })
}

/// Last path segment of a workflow ref. `gh workflow run` accepts either the
/// bare filename (`target.yml`) or the full repo-relative path
/// (`.github/workflows/target.yml`); we compare on basename so both forms
/// match the same annotation.
fn workflow_basename(s: &str) -> &str {
    s.rsplit('/').next().unwrap_or(s)
}

/// Extract the literal `<ref>` token from `gh workflow run <ref>` if present,
/// where `<ref>` is the first whitespace-delimited token after `run`. Returns
/// None when the line does not match, or when the token is empty / starts
/// with `$` / `"` / `'` (variable expansion or quoted string — documented
/// false negative).
fn extract_gh_workflow_run(line: &str) -> Option<&str> {
    // Walk tokens; first three must be exactly `gh`, `workflow`, `run`.
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "gh" {
        return None;
    }
    if tokens.next()? != "workflow" {
        return None;
    }
    if tokens.next()? != "run" {
        return None;
    }
    let target = tokens.next()?;
    // Reject variable / quoted forms; only literal targets are supported.
    if target.starts_with('$') || target.starts_with('"') || target.starts_with('\'') {
        return None;
    }
    // Strip a trailing `;` or `&&` chain start so `gh workflow run X;` still
    // resolves as `X`.
    let cleaned = target.trim_end_matches([';', ',']);
    if cleaned.is_empty() {
        return None;
    }
    Some(cleaned)
}

#[cfg(test)]
mod wiring_tests {
    use super::*;
    use std::path::PathBuf;

    fn make_ir_with_step(run_body: Option<&str>, anns: Vec<Annotation>) -> Ir {
        Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![Workflow {
                id: WorkflowId(".github/workflows/trigger.yml".into()),
                source: SourcePos {
                    file: PathBuf::from(".github/workflows/trigger.yml"),
                    line: Some(1),
                },
                name: None,
                run_name: None,
                triggers: vec![],
                jobs: vec![Job {
                    id: JobId("run".into()),
                    workflow: WorkflowId(".github/workflows/trigger.yml".into()),
                    needs: vec![],
                    permissions: None,
                    steps: vec![Step {
                        index: 0,
                        id: None,
                        name: None,
                        uses: None,
                        run: run_body.map(|s| s.to_string()),
                        if_expr: None,
                        with: Default::default(),
                        env: Default::default(),
                        shell: None,
                        working_directory: None,
                        timeout_minutes: None,
                        continue_on_error: None,
                        source: SourcePos {
                            file: PathBuf::from(".github/workflows/trigger.yml"),
                            line: Some(7),
                        },
                        annotations: anns,
                    }],
                    calls_workflow: None,
                    runs_on: None,
                    outputs: Default::default(),
                    source: SourcePos {
                        file: PathBuf::from(".github/workflows/trigger.yml"),
                        line: Some(5),
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
                }],
                permissions: None,
                defaults: None,
                env: Default::default(),
                concurrency: None,
                annotations: Vec::new(),
            }],
            actions: vec![],
            external_actions: vec![],
        }
    }

    #[test]
    fn wiring_detects_unannotated_dispatch() {
        let ir = make_ir_with_step(Some("gh workflow run target.yml"), Vec::new());
        let findings = wiring(&ir);
        assert_eq!(findings.len(), 1);
        match &findings[0].kind {
            WiringKind::UnannotatedDispatch { raw_target } => {
                assert_eq!(raw_target, "target.yml");
            }
            other => panic!("expected UnannotatedDispatch, got {other:?}"),
        }
    }

    #[test]
    fn wiring_skips_when_annotation_matches() {
        let anns = vec![Annotation {
            verb: AnnotationVerb::Dispatches,
            resolution: AnnotationResolution::Resolved {
                target: WorkflowId("target.yml".into()),
            },
            source_line: 6,
        }];
        let ir = make_ir_with_step(Some("gh workflow run target.yml"), anns);
        let findings = wiring(&ir);
        assert!(
            findings.is_empty(),
            "annotation should suppress finding: {findings:?}"
        );
    }

    #[test]
    fn wiring_skips_commented_out_invocations() {
        let body = "# gh workflow run target.yml\necho hi";
        let ir = make_ir_with_step(Some(body), Vec::new());
        let findings = wiring(&ir);
        assert!(
            findings.is_empty(),
            "commented-out gh should not be a finding: {findings:?}"
        );
    }

    #[test]
    fn wiring_emits_dangling_annotation_finding() {
        let anns = vec![Annotation {
            verb: AnnotationVerb::Dispatches,
            resolution: AnnotationResolution::Dangling {
                raw_target: "../bad".into(),
                reason: "path must not contain `..`, `.`, or empty segments".into(),
            },
            source_line: 4,
        }];
        let ir = make_ir_with_step(None, anns);
        let findings = wiring(&ir);
        assert_eq!(findings.len(), 1);
        match &findings[0].kind {
            WiringKind::DanglingAnnotation { raw_target, reason } => {
                assert_eq!(raw_target, "../bad");
                assert!(reason.contains("`..`"));
            }
            other => panic!("expected DanglingAnnotation, got {other:?}"),
        }
    }

    #[test]
    fn wiring_emits_dangling_annotation_for_composite_carrier() {
        // Both manifest-level and composite-step Dangling annotations must
        // surface as DanglingAnnotation findings (issue #110).
        let action_path = PathBuf::from(".github/actions/notify/action.yaml");
        let ir = Ir {
            schema_version: 3,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![],
            actions: vec![LocalAction {
                id: ActionId(".github/actions/notify".into()),
                source: SourcePos {
                    file: action_path.clone(),
                    line: Some(1),
                },
                name: None,
                kind: ActionKind::Composite,
                inputs: vec![],
                outputs: vec![],
                steps: vec![Step {
                    index: 0,
                    id: None,
                    name: None,
                    uses: None,
                    run: Some(":".into()),
                    if_expr: None,
                    with: Default::default(),
                    env: Default::default(),
                    shell: Some("bash".into()),
                    working_directory: None,
                    timeout_minutes: None,
                    continue_on_error: None,
                    source: SourcePos {
                        file: action_path.clone(),
                        line: Some(7),
                    },
                    annotations: vec![Annotation {
                        verb: AnnotationVerb::Dispatches,
                        resolution: AnnotationResolution::Dangling {
                            raw_target: "scripts/bad-step.sh".into(),
                            reason: "must be a workflow under .github/workflows/".into(),
                        },
                        source_line: 6,
                    }],
                }],
                annotations: vec![Annotation {
                    verb: AnnotationVerb::Triggers,
                    resolution: AnnotationResolution::Dangling {
                        raw_target: "scripts/bad-manifest.sh".into(),
                        reason: "must be a workflow under .github/workflows/".into(),
                    },
                    source_line: 1,
                }],
            }],
            external_actions: vec![],
        };
        let findings = wiring(&ir);
        let dangling: Vec<&WiringFinding> = findings
            .iter()
            .filter(|f| matches!(f.kind, WiringKind::DanglingAnnotation { .. }))
            .collect();
        assert_eq!(
            dangling.len(),
            2,
            "expected 2 dangling annotation findings (manifest + step), got: {findings:?}"
        );
        let raw_targets: Vec<&str> = dangling
            .iter()
            .filter_map(|f| match &f.kind {
                WiringKind::DanglingAnnotation { raw_target, .. } => Some(raw_target.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            raw_targets.contains(&"scripts/bad-manifest.sh"),
            "manifest-level dangling annotation must surface: {raw_targets:?}"
        );
        assert!(
            raw_targets.contains(&"scripts/bad-step.sh"),
            "composite-step dangling annotation must surface: {raw_targets:?}"
        );
    }

    #[test]
    fn wiring_emits_dangling_workflow_run_for_unresolvable_name() {
        // Build an IR with one workflow_run trigger whose upstream name does
        // not match any local workflow — wiring must emit DanglingWorkflowRun.
        let ir = Ir {
            schema_version: 3,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![Workflow {
                id: WorkflowId(".github/workflows/consumer.yml".into()),
                source: SourcePos {
                    file: PathBuf::from(".github/workflows/consumer.yml"),
                    line: Some(1),
                },
                name: Some("Consumer".into()),
                run_name: None,
                triggers: vec![TriggerSpec {
                    event: EventKind::WorkflowRun,
                    branches: RefFilter::None,
                    tags: RefFilter::None,
                    paths: RefFilter::None,
                    types: None,
                    extras: Some(EventExtras::WorkflowRun {
                        workflows: vec!["GhostWorkflow".into()],
                    }),
                }],
                jobs: vec![],
                permissions: None,
                defaults: None,
                env: Default::default(),
                concurrency: None,
                annotations: vec![],
            }],
            actions: vec![],
            external_actions: vec![],
        };
        let findings = wiring(&ir);
        assert_eq!(
            findings.len(),
            1,
            "expected one DanglingWorkflowRun finding, got: {findings:?}"
        );
        match &findings[0].kind {
            WiringKind::DanglingWorkflowRun { raw_name } => {
                assert_eq!(raw_name, "GhostWorkflow");
            }
            other => panic!("expected DanglingWorkflowRun, got {other:?}"),
        }
    }

    #[test]
    fn wiring_no_finding_when_workflow_run_name_resolves() {
        // Both workflows present; consumer's workflow_run.workflows matches the
        // upstream by name. No DanglingWorkflowRun should be emitted.
        let ir = Ir {
            schema_version: 3,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                Workflow {
                    id: WorkflowId(".github/workflows/trigger.yml".into()),
                    source: SourcePos {
                        file: PathBuf::from(".github/workflows/trigger.yml"),
                        line: Some(1),
                    },
                    name: Some("Trigger".into()),
                    run_name: None,
                    triggers: vec![TriggerSpec::bare(EventKind::Push)],
                    jobs: vec![],
                    permissions: None,
                    defaults: None,
                    env: Default::default(),
                    concurrency: None,
                    annotations: vec![],
                },
                Workflow {
                    id: WorkflowId(".github/workflows/consumer.yml".into()),
                    source: SourcePos {
                        file: PathBuf::from(".github/workflows/consumer.yml"),
                        line: Some(1),
                    },
                    name: Some("Consumer".into()),
                    run_name: None,
                    triggers: vec![TriggerSpec {
                        event: EventKind::WorkflowRun,
                        branches: RefFilter::None,
                        tags: RefFilter::None,
                        paths: RefFilter::None,
                        types: None,
                        extras: Some(EventExtras::WorkflowRun {
                            workflows: vec!["Trigger".into()],
                        }),
                    }],
                    jobs: vec![],
                    permissions: None,
                    defaults: None,
                    env: Default::default(),
                    concurrency: None,
                    annotations: vec![],
                },
            ],
            actions: vec![],
            external_actions: vec![],
        };
        let findings = wiring(&ir);
        let dangling_wr: Vec<_> = findings
            .iter()
            .filter(|f| matches!(f.kind, WiringKind::DanglingWorkflowRun { .. }))
            .collect();
        assert!(
            dangling_wr.is_empty(),
            "resolved name must not produce a finding: {findings:?}"
        );
    }

    #[test]
    fn wiring_no_finding_when_workflow_run_resolves_via_path_fallback() {
        // Upstream has no name:; path acts as the key. Consumer uses the path
        // directly in workflows: [...] — should resolve without a finding.
        let upstream_path = ".github/workflows/nightly.yml";
        let ir = Ir {
            schema_version: 3,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                Workflow {
                    id: WorkflowId(upstream_path.into()),
                    source: SourcePos {
                        file: PathBuf::from(upstream_path),
                        line: Some(1),
                    },
                    name: None, // no name: field → path is the effective name
                    run_name: None,
                    triggers: vec![TriggerSpec::bare(EventKind::Push)],
                    jobs: vec![],
                    permissions: None,
                    defaults: None,
                    env: Default::default(),
                    concurrency: None,
                    annotations: vec![],
                },
                Workflow {
                    id: WorkflowId(".github/workflows/consumer.yml".into()),
                    source: SourcePos {
                        file: PathBuf::from(".github/workflows/consumer.yml"),
                        line: Some(1),
                    },
                    name: None,
                    run_name: None,
                    triggers: vec![TriggerSpec {
                        event: EventKind::WorkflowRun,
                        branches: RefFilter::None,
                        tags: RefFilter::None,
                        paths: RefFilter::None,
                        types: None,
                        extras: Some(EventExtras::WorkflowRun {
                            workflows: vec![upstream_path.into()],
                        }),
                    }],
                    jobs: vec![],
                    permissions: None,
                    defaults: None,
                    env: Default::default(),
                    concurrency: None,
                    annotations: vec![],
                },
            ],
            actions: vec![],
            external_actions: vec![],
        };
        let findings = wiring(&ir);
        let dangling_wr: Vec<_> = findings
            .iter()
            .filter(|f| matches!(f.kind, WiringKind::DanglingWorkflowRun { .. }))
            .collect();
        assert!(
            dangling_wr.is_empty(),
            "path-fallback resolution must not produce a finding: {findings:?}"
        );
    }

    /// Helper: minimal workflow shell with a single named job. The caller
    /// fills the job's calls_workflow / step uses to drive specific branches.
    fn workflow_with_job(file: &str, job: Job) -> Workflow {
        Workflow {
            id: WorkflowId(file.into()),
            source: SourcePos {
                file: PathBuf::from(file),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![],
            jobs: vec![job],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: vec![],
        }
    }

    fn empty_job(id: &str, file: &str) -> Job {
        Job {
            id: JobId(id.into()),
            workflow: WorkflowId(file.into()),
            needs: vec![],
            permissions: None,
            steps: vec![],
            calls_workflow: None,
            runs_on: None,
            outputs: Default::default(),
            source: SourcePos {
                file: PathBuf::from(file),
                line: Some(5),
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

    fn empty_step(line: usize, file: &str) -> Step {
        Step {
            index: 0,
            id: None,
            name: None,
            uses: None,
            run: None,
            if_expr: None,
            with: Default::default(),
            env: Default::default(),
            shell: None,
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
            source: SourcePos {
                file: PathBuf::from(file),
                line: Some(line),
            },
            annotations: Vec::new(),
        }
    }

    #[test]
    fn wiring_emits_dangling_for_job_level_local_workflow_call_to_unknown_target() {
        let file = ".github/workflows/caller.yml";
        let mut job = empty_job("call", file);
        // workflow_call wires job.calls_workflow with WorkflowRef::Local(target);
        // because the target does not exist in `ir.workflows`, this must
        // surface as DanglingLocalUses { local_kind: Workflow, .. }.
        job.calls_workflow = Some(CallsWorkflow {
            workflow_ref: WorkflowRef::Local(WorkflowId(".github/workflows/missing.yml".into())),
            with: Default::default(),
            secrets: SecretsPass::None,
        });
        let ir = Ir {
            schema_version: 3,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![workflow_with_job(file, job)],
            actions: vec![],
            external_actions: vec![],
        };
        let findings = wiring(&ir);
        assert!(findings.iter().any(|f| matches!(
            &f.kind,
            WiringKind::DanglingLocalUses {
                local_kind: DanglingLocalUsesKind::Workflow,
                raw_target,
            } if raw_target == ".github/workflows/missing.yml"
        )));
    }

    #[test]
    fn wiring_emits_dangling_for_step_level_local_workflow_uses_to_unknown_target() {
        let file = ".github/workflows/caller.yml";
        let mut step = empty_step(7, file);
        step.uses = Some(UsesRef::LocalWorkflow(WorkflowId(
            ".github/workflows/missing-step.yml".into(),
        )));
        let mut job = empty_job("call", file);
        job.steps = vec![step];
        let ir = Ir {
            schema_version: 3,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![workflow_with_job(file, job)],
            actions: vec![],
            external_actions: vec![],
        };
        let findings = wiring(&ir);
        assert!(findings.iter().any(|f| matches!(
            &f.kind,
            WiringKind::DanglingLocalUses {
                local_kind: DanglingLocalUsesKind::Workflow,
                raw_target,
            } if raw_target == ".github/workflows/missing-step.yml"
        )));
    }

    #[test]
    fn wiring_emits_dangling_for_composite_action_step_uses_to_unknown_targets() {
        let action_file = ".github/actions/composite/action.yml";
        let mut step_action = empty_step(7, action_file);
        step_action.uses = Some(UsesRef::LocalAction(ActionId(
            ".github/actions/missing".into(),
        )));
        let mut step_wf = empty_step(9, action_file);
        step_wf.uses = Some(UsesRef::LocalWorkflow(WorkflowId(
            ".github/workflows/missing.yml".into(),
        )));
        let ir = Ir {
            schema_version: 3,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![],
            actions: vec![LocalAction {
                id: ActionId(".github/actions/composite".into()),
                source: SourcePos {
                    file: PathBuf::from(action_file),
                    line: Some(1),
                },
                name: None,
                kind: ActionKind::Composite,
                inputs: vec![],
                outputs: vec![],
                steps: vec![step_action, step_wf],
                annotations: vec![],
            }],
            external_actions: vec![],
        };
        let findings = wiring(&ir);
        // Expect both an Action-kind and a Workflow-kind DanglingLocalUses.
        let action_finding = findings.iter().any(|f| {
            matches!(
                &f.kind,
                WiringKind::DanglingLocalUses {
                    local_kind: DanglingLocalUsesKind::Action,
                    ..
                }
            )
        });
        let workflow_finding = findings.iter().any(|f| {
            matches!(
                &f.kind,
                WiringKind::DanglingLocalUses {
                    local_kind: DanglingLocalUsesKind::Workflow,
                    ..
                }
            )
        });
        assert!(action_finding, "missing local action ref must surface");
        assert!(workflow_finding, "missing local workflow ref must surface");
    }

    #[test]
    fn wiring_skips_run_body_lines_starting_with_ravelact_marker() {
        // Lines matching the `ravelact:` annotation marker (the same marker
        // checked by `line_starts_with_ravelact`) inside a step's run body
        // must be skipped — they belong to the annotation parser, not the
        // wiring scan.
        let body = "# ravelact: dispatches=foo.yml\ngh workflow run foo.yml";
        let anns = vec![Annotation {
            verb: AnnotationVerb::Dispatches,
            resolution: AnnotationResolution::Resolved {
                target: WorkflowId("foo.yml".into()),
            },
            source_line: 4,
        }];
        let ir = make_ir_with_step(Some(body), anns);
        let findings = wiring(&ir);
        assert!(
            findings.is_empty(),
            "ravelact-marker line must be skipped; got {findings:?}",
        );
    }

    #[test]
    fn extract_gh_workflow_run_rejects_non_matching_prefixes() {
        // Second token must be exactly `workflow`.
        assert_eq!(extract_gh_workflow_run("gh action run target.yml"), None);
        // Third token must be exactly `run`.
        assert_eq!(extract_gh_workflow_run("gh workflow list target.yml"), None);
        // Variable expansion / quoted forms are explicit false negatives.
        assert_eq!(extract_gh_workflow_run("gh workflow run $TARGET"), None);
        assert_eq!(
            extract_gh_workflow_run("gh workflow run \"target.yml\""),
            None,
        );
        assert_eq!(
            extract_gh_workflow_run("gh workflow run 'target.yml'"),
            None
        );
        // Trailing punctuation collapses to empty.
        assert_eq!(extract_gh_workflow_run("gh workflow run ;"), None);
    }

    #[test]
    fn step_has_dispatch_annotation_returns_false_for_non_resolved_or_wrong_verb() {
        // Dangling Dispatches annotation must not match (only Resolved does).
        let anns_dangling = vec![Annotation {
            verb: AnnotationVerb::Dispatches,
            resolution: AnnotationResolution::Dangling {
                raw_target: "x".into(),
                reason: "r".into(),
            },
            source_line: 4,
        }];
        assert!(!step_has_dispatch_annotation(&anns_dangling, "x"));
        // Resolved annotation with a different verb (e.g. Triggers)
        // must not match — only `AnnotationVerb::Dispatches` counts.
        let anns_wrong_verb = vec![Annotation {
            verb: AnnotationVerb::Triggers,
            resolution: AnnotationResolution::Resolved {
                target: WorkflowId("x".into()),
            },
            source_line: 4,
        }];
        assert!(!step_has_dispatch_annotation(&anns_wrong_verb, "x"));
    }
}
