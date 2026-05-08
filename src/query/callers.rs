use crate::ir::*;
use crate::query::Target;
use serde::Serialize;

/// Anchor location of an `# ravelact:dispatches` / `# ravelact:triggers`
/// annotation that resolved to the queried target.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum AnnotationAnchor {
    Workflow,
    Job { job: String },
    Step { job: String, step_index: usize },
}

/// Anchor location of an `# ravelact:dispatches` / `# ravelact:triggers`
/// annotation carried inside a composite action manifest. Mirrors
/// [`AnnotationAnchor`] for the workflow side.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum CompositeAnnotationAnchor {
    /// Annotation attached to the action manifest itself (file-head).
    Action,
    /// Annotation attached to a `runs.steps[i]` entry.
    Step { step_index: usize },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum CallerHit {
    /// Job-level `uses:` calling a reusable workflow.
    JobCall { workflow: WorkflowId, job: String },
    /// Step-level `uses:` inside a workflow.
    Step {
        workflow: WorkflowId,
        job: String,
        step_index: usize,
    },
    /// Step-level `uses:` inside a composite action.
    CompositeStep { action: ActionId, step_index: usize },
    /// `# ravelact:<verb> <target>` annotation pointing at the queried workflow,
    /// carried by a workflow / job / workflow-side step.
    Annotated {
        workflow: WorkflowId,
        anchor: AnnotationAnchor,
        verb: AnnotationVerb,
    },
    /// `# ravelact:<verb> <target>` annotation pointing at the queried workflow,
    /// carried by a composite action manifest or by one of its `runs.steps`.
    AnnotatedComposite {
        action: ActionId,
        anchor: CompositeAnnotationAnchor,
        verb: AnnotationVerb,
    },
}

pub fn callers(ir: &Ir, target: &Target) -> Vec<CallerHit> {
    let mut hits = Vec::new();

    for wf in &ir.workflows {
        // Workflow-level annotations
        if let Target::Workflow(t) = target {
            for ann in &wf.annotations {
                if annotation_matches(ann, t) {
                    hits.push(CallerHit::Annotated {
                        workflow: wf.id.clone(),
                        anchor: AnnotationAnchor::Workflow,
                        verb: ann.verb,
                    });
                }
            }
        }

        for job in &wf.jobs {
            // job-level reusable workflow call
            if let (Target::Workflow(target_wf), Some(call)) = (target, &job.calls_workflow) {
                if let WorkflowRef::Local(local) = &call.workflow_ref {
                    if local.0 == target_wf.0 {
                        hits.push(CallerHit::JobCall {
                            workflow: wf.id.clone(),
                            job: job.id.0.clone(),
                        });
                    }
                }
            }
            // job-level annotations
            if let Target::Workflow(t) = target {
                for ann in &job.annotations {
                    if annotation_matches(ann, t) {
                        hits.push(CallerHit::Annotated {
                            workflow: wf.id.clone(),
                            anchor: AnnotationAnchor::Job {
                                job: job.id.0.clone(),
                            },
                            verb: ann.verb,
                        });
                    }
                }
            }
            // step-level uses + annotations
            for step in &job.steps {
                if step_matches(step, target) {
                    hits.push(CallerHit::Step {
                        workflow: wf.id.clone(),
                        job: job.id.0.clone(),
                        step_index: step.index,
                    });
                }
                if let Target::Workflow(t) = target {
                    for ann in &step.annotations {
                        if annotation_matches(ann, t) {
                            hits.push(CallerHit::Annotated {
                                workflow: wf.id.clone(),
                                anchor: AnnotationAnchor::Step {
                                    job: job.id.0.clone(),
                                    step_index: step.index,
                                },
                                verb: ann.verb,
                            });
                        }
                    }
                }
            }
        }
    }

    for action in &ir.actions {
        // Composite-action manifest-level annotations.
        if let Target::Workflow(t) = target {
            for ann in &action.annotations {
                if annotation_matches(ann, t) {
                    hits.push(CallerHit::AnnotatedComposite {
                        action: action.id.clone(),
                        anchor: CompositeAnnotationAnchor::Action,
                        verb: ann.verb,
                    });
                }
            }
        }
        for step in &action.steps {
            if step_matches(step, target) {
                hits.push(CallerHit::CompositeStep {
                    action: action.id.clone(),
                    step_index: step.index,
                });
            }
            if let Target::Workflow(t) = target {
                for ann in &step.annotations {
                    if annotation_matches(ann, t) {
                        hits.push(CallerHit::AnnotatedComposite {
                            action: action.id.clone(),
                            anchor: CompositeAnnotationAnchor::Step {
                                step_index: step.index,
                            },
                            verb: ann.verb,
                        });
                    }
                }
            }
        }
    }

    hits
}

fn annotation_matches(ann: &Annotation, target: &WorkflowId) -> bool {
    matches!(
        &ann.resolution,
        AnnotationResolution::Resolved { target: t } if t.0 == target.0
    )
}

fn step_matches(step: &Step, target: &Target) -> bool {
    let Some(uses) = step.uses.as_ref() else {
        return false;
    };
    match (target, uses) {
        (Target::Workflow(t), UsesRef::LocalWorkflow(WorkflowId(p))) => &t.0 == p,
        (Target::Action(t), UsesRef::LocalAction(ActionId(p))) => &t.0 == p,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_ir() -> Ir {
        let wf_id = WorkflowId(".github/workflows/ci.yml".into());
        let build_id = WorkflowId(".github/workflows/build.yml".into());
        let action_id = ActionId(".github/actions/setup".into());

        let job = Job {
            id: JobId("test".into()),
            workflow: wf_id.clone(),
            needs: vec![],
            permissions: None,
            steps: vec![Step {
                index: 0,
                id: None,
                name: None,
                uses: Some(UsesRef::LocalAction(action_id.clone())),
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
            }],
            calls_workflow: None,
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
        };
        let job2 = Job {
            id: JobId("build".into()),
            workflow: wf_id.clone(),
            needs: vec![],
            permissions: None,
            steps: vec![],
            calls_workflow: Some(CallsWorkflow {
                workflow_ref: WorkflowRef::Local(build_id.clone()),
                with: Default::default(),
                secrets: SecretsPass::Inherit,
            }),
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
        };

        Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                Workflow {
                    id: wf_id.clone(),
                    source: SourcePos {
                        file: PathBuf::new(),
                        line: None,
                    },
                    name: None,
                    run_name: None,
                    triggers: vec![],
                    jobs: vec![job, job2],
                    permissions: None,
                    defaults: None,
                    env: Default::default(),
                    concurrency: None,
                    annotations: Vec::new(),
                },
                Workflow {
                    id: build_id.clone(),
                    source: SourcePos {
                        file: PathBuf::new(),
                        line: None,
                    },
                    name: None,
                    run_name: None,
                    triggers: vec![TriggerSpec::bare(EventKind::WorkflowCall)],
                    jobs: vec![],
                    permissions: None,
                    defaults: None,
                    env: Default::default(),
                    concurrency: None,
                    annotations: Vec::new(),
                },
            ],
            actions: vec![LocalAction {
                id: action_id.clone(),
                source: SourcePos {
                    file: PathBuf::new(),
                    line: None,
                },
                name: None,
                kind: ActionKind::Composite,
                inputs: vec![],
                outputs: vec![],
                steps: vec![],
                annotations: Vec::new(),
            }],
            external_actions: vec![],
        }
    }

    #[test]
    fn callers_finds_local_action_step_and_job_call() {
        let ir = make_ir();

        let hits_action = callers(&ir, &Target::from_user_input(".github/actions/setup"));
        assert_eq!(hits_action.len(), 1);
        match &hits_action[0] {
            CallerHit::Step {
                workflow,
                job,
                step_index,
            } => {
                assert_eq!(workflow.0, ".github/workflows/ci.yml");
                assert_eq!(job, "test");
                assert_eq!(*step_index, 0);
            }
            other => panic!("expected Step, got {other:?}"),
        }

        let hits_wf = callers(&ir, &Target::from_user_input(".github/workflows/build.yml"));
        assert_eq!(hits_wf.len(), 1);
        match &hits_wf[0] {
            CallerHit::JobCall { workflow, job } => {
                assert_eq!(workflow.0, ".github/workflows/ci.yml");
                assert_eq!(job, "build");
            }
            other => panic!("expected JobCall, got {other:?}"),
        }
    }

    #[test]
    fn callers_finds_annotated_dispatch() {
        // Workflow with a Step-anchored `dispatches` annotation pointing at
        // build.yml — `callers build.yml` must surface a `CallerHit::Annotated`.
        let trigger_id = WorkflowId(".github/workflows/trigger.yml".into());
        let target_id = WorkflowId(".github/workflows/build.yml".into());

        let trigger_wf = Workflow {
            id: trigger_id.clone(),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![],
            jobs: vec![Job {
                id: JobId("run".into()),
                workflow: trigger_id.clone(),
                needs: vec![],
                permissions: None,
                steps: vec![Step {
                    index: 0,
                    id: None,
                    name: None,
                    uses: None,
                    run: Some("gh workflow run build.yml".into()),
                    if_expr: None,
                    with: Default::default(),
                    env: Default::default(),
                    shell: None,
                    working_directory: None,
                    timeout_minutes: None,
                    continue_on_error: None,
                    source: SourcePos {
                        file: PathBuf::new(),
                        line: Some(7),
                    },
                    annotations: vec![Annotation {
                        verb: AnnotationVerb::Dispatches,
                        resolution: AnnotationResolution::Resolved {
                            target: target_id.clone(),
                        },
                        source_line: 6,
                    }],
                }],
                calls_workflow: None,
                runs_on: None,
                outputs: Default::default(),
                source: SourcePos {
                    file: PathBuf::new(),
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
        };
        let target_wf = Workflow {
            id: target_id.clone(),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: Vec::new(),
        };
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![trigger_wf, target_wf],
            actions: vec![],
            external_actions: vec![],
        };

        let hits = callers(&ir, &Target::from_user_input(".github/workflows/build.yml"));
        assert_eq!(hits.len(), 1);
        match &hits[0] {
            CallerHit::Annotated {
                workflow,
                anchor,
                verb,
            } => {
                assert_eq!(workflow.0, ".github/workflows/trigger.yml");
                assert!(matches!(
                    anchor,
                    AnnotationAnchor::Step { job, step_index: 0 } if job == "run"
                ));
                assert_eq!(*verb, AnnotationVerb::Dispatches);
            }
            other => panic!("expected Annotated, got {other:?}"),
        }
    }

    #[test]
    fn callers_finds_annotated_composite_carriers() {
        // Composite manifest-level + composite-step annotations targeting
        // build.yml must surface as `CallerHit::AnnotatedComposite` (issue #110).
        let target_id = WorkflowId(".github/workflows/build.yml".into());
        let action_id = ActionId(".github/actions/notify".into());
        let step = Step {
            index: 0,
            id: None,
            name: None,
            uses: None,
            run: Some("gh workflow run build.yml".into()),
            if_expr: None,
            with: Default::default(),
            env: Default::default(),
            shell: Some("bash".into()),
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(7),
            },
            annotations: vec![Annotation {
                verb: AnnotationVerb::Dispatches,
                resolution: AnnotationResolution::Resolved {
                    target: target_id.clone(),
                },
                source_line: 6,
            }],
        };
        let action = LocalAction {
            id: action_id.clone(),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            kind: ActionKind::Composite,
            inputs: vec![],
            outputs: vec![],
            steps: vec![step],
            annotations: vec![Annotation {
                verb: AnnotationVerb::Triggers,
                resolution: AnnotationResolution::Resolved {
                    target: target_id.clone(),
                },
                source_line: 1,
            }],
        };
        let target_wf = Workflow {
            id: target_id.clone(),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: Vec::new(),
        };
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![target_wf],
            actions: vec![action],
            external_actions: vec![],
        };

        let hits = callers(&ir, &Target::from_user_input(".github/workflows/build.yml"));
        let composite_hits: Vec<&CallerHit> = hits
            .iter()
            .filter(|h| matches!(h, CallerHit::AnnotatedComposite { .. }))
            .collect();
        assert_eq!(
            composite_hits.len(),
            2,
            "expected 2 AnnotatedComposite hits (manifest + step), got: {hits:?}"
        );
        let mut anchors: Vec<&CompositeAnnotationAnchor> = composite_hits
            .iter()
            .filter_map(|h| match h {
                CallerHit::AnnotatedComposite { anchor, .. } => Some(anchor),
                _ => None,
            })
            .collect();
        anchors.sort_by_key(|a| match a {
            CompositeAnnotationAnchor::Action => 0,
            CompositeAnnotationAnchor::Step { .. } => 1,
        });
        assert!(matches!(anchors[0], CompositeAnnotationAnchor::Action));
        assert!(matches!(
            anchors[1],
            CompositeAnnotationAnchor::Step { step_index: 0 }
        ));
    }

    /// Build a workflow whose only step uses the given action id.
    fn wf_using_action(wf_id: &str, action_id: &str) -> Workflow {
        let step = Step {
            index: 0,
            id: None,
            name: None,
            uses: Some(UsesRef::LocalAction(ActionId(action_id.into()))),
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
        };
        let job = Job {
            id: JobId("run".into()),
            workflow: WorkflowId(wf_id.into()),
            needs: vec![],
            permissions: None,
            steps: vec![step],
            calls_workflow: None,
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
        };
        Workflow {
            id: WorkflowId(wf_id.into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![job],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: Vec::new(),
        }
    }

    #[test]
    fn callers_returns_empty_when_target_has_no_callers() {
        // build.yml is workflow_call-only and nothing references it.
        let target_id = WorkflowId(".github/workflows/build.yml".into());
        let target_wf = Workflow {
            id: target_id.clone(),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: Vec::new(),
        };
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![target_wf],
            actions: vec![],
            external_actions: vec![],
        };
        let hits = callers(&ir, &Target::from_user_input(".github/workflows/build.yml"));
        assert!(hits.is_empty(), "expected no callers, got {hits:?}");
    }

    #[test]
    fn callers_action_target_skips_workflow_target_only_branches() {
        // Target::Action must NOT match workflow callsites or annotations,
        // even when the IR has both. Pins the target-discriminator branches
        // in `step_matches` and `annotation_matches` callers.
        let action_id = ActionId(".github/actions/setup".into());
        let other_action = ActionId(".github/actions/lint".into());
        let action = LocalAction {
            id: action_id.clone(),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            kind: ActionKind::Composite,
            inputs: vec![],
            outputs: vec![],
            steps: vec![],
            annotations: Vec::new(),
        };
        let other = LocalAction {
            id: other_action.clone(),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            kind: ActionKind::Composite,
            inputs: vec![],
            outputs: vec![],
            steps: vec![],
            annotations: Vec::new(),
        };
        // Workflow that uses the *other* action — should not match Target::Action(setup).
        let wf = wf_using_action(".github/workflows/ci.yml", ".github/actions/lint");
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![wf],
            actions: vec![action, other],
            external_actions: vec![],
        };
        let hits = callers(&ir, &Target::from_user_input(".github/actions/setup"));
        assert!(
            hits.is_empty(),
            "Action target must be path-discriminated, got {hits:?}"
        );
    }

    #[test]
    fn callers_finds_three_distinct_workflow_callers_for_one_action() {
        // Multi-caller scenario (≥3 callers point at one target): three
        // workflows each have a step using `.github/actions/setup`. All three
        // must surface as `CallerHit::Step` entries in the result.
        let action_id = ActionId(".github/actions/setup".into());
        let action = LocalAction {
            id: action_id.clone(),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            kind: ActionKind::Composite,
            inputs: vec![],
            outputs: vec![],
            steps: vec![],
            annotations: Vec::new(),
        };
        let a = wf_using_action(".github/workflows/a.yml", ".github/actions/setup");
        let b = wf_using_action(".github/workflows/b.yml", ".github/actions/setup");
        let c = wf_using_action(".github/workflows/c.yml", ".github/actions/setup");
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![a, b, c],
            actions: vec![action],
            external_actions: vec![],
        };
        let hits = callers(&ir, &Target::from_user_input(".github/actions/setup"));
        assert_eq!(hits.len(), 3, "expected 3 distinct callers, got {hits:?}");
        let mut workflow_ids: Vec<&str> = hits
            .iter()
            .map(|h| match h {
                CallerHit::Step { workflow, .. } => workflow.0.as_str(),
                other => panic!("expected Step, got {other:?}"),
            })
            .collect();
        workflow_ids.sort();
        assert_eq!(
            workflow_ids,
            vec![
                ".github/workflows/a.yml",
                ".github/workflows/b.yml",
                ".github/workflows/c.yml",
            ],
        );
    }
}
