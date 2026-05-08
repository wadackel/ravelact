use crate::ir::{EventKind, Ir, SecretsPass, Workflow, WorkflowRef};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Finding {
    #[serde(flatten)]
    pub kind: FindingKind,
    pub severity: Severity,
    pub location: FindingLocation,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum FindingKind {
    /// Caller did not pass a secret that the directly-called callee declares
    /// as `required: true`. Always depth = 1 (entry → first callee). HIGH.
    MissingSecretPropagation {
        caller: String,
        caller_job: String,
        callee: String,
        secret: String,
    },
    /// Depth ≥ 2: the leaf callee in the chain declares the secret as
    /// `required: true`, but somewhere along the chain a layer dropped it
    /// (either by switching to an explicit map missing the key, or by
    /// declaring no `secrets:` at all). HIGH.
    SecretsInheritChainBreak {
        entry: String,
        chain: Vec<String>,
        dropped_at: String,
        secret: String,
    },
    /// A reusable workflow callee reached from an entry-point chain has a
    /// job-level `environment:`. Per GHA spec the job's env secrets shadow
    /// any caller-passed secrets of the same name. MEDIUM.
    EnvironmentInWorkflowCallCallee {
        workflow: String,
        job: String,
        environment: String,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "scope")]
pub enum FindingLocation {
    #[allow(dead_code)]
    Workflow { file: PathBuf },
    Job {
        file: PathBuf,
        workflow: String,
        job: String,
    },
}

impl Finding {
    fn kind_name(&self) -> &'static str {
        match &self.kind {
            FindingKind::MissingSecretPropagation { .. } => "missing-secret-propagation",
            FindingKind::SecretsInheritChainBreak { .. } => "secrets-inherit-chain-break",
            FindingKind::EnvironmentInWorkflowCallCallee { .. } => {
                "environment-in-workflow-call-callee"
            }
        }
    }

    fn severity_name(&self) -> &'static str {
        match self.severity {
            Severity::High => "high",
            Severity::Medium => "medium",
        }
    }
}

impl std::fmt::Display for Finding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let loc = match &self.location {
            FindingLocation::Workflow { file } => format!("{}", file.display()),
            FindingLocation::Job {
                file,
                workflow: _,
                job,
            } => format!("{}:{}", file.display(), job),
        };
        write!(
            f,
            "{}  {}  {}  {}",
            loc,
            self.severity_name(),
            self.kind_name(),
            self.message
        )
    }
}

/// Path-local "currently reachable secret name set". Entry-points start at
/// `All` (the workflow has access to every repository / org secret); each
/// hop's `secrets:` declaration narrows or preserves it per GHA spec.
#[derive(Debug, Clone)]
enum Reachable {
    /// Any secret name is in scope (entry-point baseline, or after an
    /// `Inherit` hop from another `All` state).
    All,
    /// Only these specific names propagate further. `Names(empty)` means
    /// the chain has been fully cut (caller declared no secrets).
    Names(BTreeSet<String>),
}

impl Reachable {
    fn contains(&self, name: &str) -> bool {
        match self {
            Reachable::All => true,
            Reachable::Names(s) => s.contains(name),
        }
    }

    fn apply(&self, pass: &SecretsPass) -> Reachable {
        match pass {
            SecretsPass::Inherit => self.clone(),
            SecretsPass::Explicit(map) => {
                let keys: BTreeSet<String> = map.keys().cloned().collect();
                match self {
                    Reachable::All => Reachable::Names(keys),
                    Reachable::Names(current) => {
                        Reachable::Names(current.intersection(&keys).cloned().collect())
                    }
                }
            }
            SecretsPass::None => Reachable::Names(BTreeSet::new()),
        }
    }
}

#[derive(Debug, Clone)]
struct Hop {
    caller_workflow: String,
    caller_job: String,
    callee_workflow: String,
    pass: SecretsPass,
}

pub fn check(ir: &Ir) -> Vec<Finding> {
    let workflow_by_id: BTreeMap<&str, &Workflow> =
        ir.workflows.iter().map(|w| (w.id.0.as_str(), w)).collect();

    let mut findings: Vec<Finding> = Vec::new();
    let mut emitted: BTreeSet<String> = BTreeSet::new();

    for wf in &ir.workflows {
        let is_entry = wf.triggers.iter().any(|t| t.is_entry_point());
        if !is_entry {
            continue;
        }
        let mut path: Vec<Hop> = Vec::new();
        let mut visiting: BTreeSet<String> = BTreeSet::new();
        visiting.insert(wf.id.0.clone());
        dfs(
            wf,
            &workflow_by_id,
            &mut path,
            &mut visiting,
            &mut findings,
            &mut emitted,
        );
    }

    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| location_key(&a.location).cmp(&location_key(&b.location)))
            .then_with(|| kind_discriminator(&a.kind).cmp(&kind_discriminator(&b.kind)))
    });

    findings
}

fn dfs(
    current: &Workflow,
    workflow_by_id: &BTreeMap<&str, &Workflow>,
    path: &mut Vec<Hop>,
    visiting: &mut BTreeSet<String>,
    findings: &mut Vec<Finding>,
    emitted: &mut BTreeSet<String>,
) {
    // (c) EnvironmentInWorkflowCallCallee — only when reached as callee
    // (path non-empty) and the callee is reusable (has workflow_call trigger).
    if !path.is_empty()
        && current
            .triggers
            .iter()
            .any(|t| t.event == EventKind::WorkflowCall)
    {
        for job in &current.jobs {
            if let Some(env) = job.environment.as_ref() {
                let key = format!("Env|{}|{}", current.id.0, job.id.0);
                if emitted.insert(key) {
                    findings.push(Finding {
                        kind: FindingKind::EnvironmentInWorkflowCallCallee {
                            workflow: current.id.0.clone(),
                            job: job.id.0.clone(),
                            environment: env.name.clone(),
                        },
                        severity: Severity::Medium,
                        location: FindingLocation::Job {
                            file: current.source.file.clone(),
                            workflow: current.id.0.clone(),
                            job: job.id.0.clone(),
                        },
                        message: format!(
                            "reusable callee `{}` job `{}` declares `environment: {}` — env-scoped secrets shadow caller-passed secrets per GHA spec",
                            current.id.0, job.id.0, env.name
                        ),
                    });
                }
            }
        }
    }

    // Walk every job that calls a reusable workflow.
    for job in &current.jobs {
        let Some(call) = &job.calls_workflow else {
            continue;
        };
        let WorkflowRef::Local(callee_id) = &call.workflow_ref else {
            continue;
        };
        let Some(callee) = workflow_by_id.get(callee_id.0.as_str()) else {
            continue;
        };

        let hop = Hop {
            caller_workflow: current.id.0.clone(),
            caller_job: job.id.0.clone(),
            callee_workflow: callee.id.0.clone(),
            pass: call.secrets.clone(),
        };

        // Compute reachable set after each hop along path + this hop.
        let mut reachables: Vec<Reachable> = vec![Reachable::All];
        for h in path.iter() {
            let next = reachables.last().unwrap().apply(&h.pass);
            reachables.push(next);
        }
        reachables.push(reachables.last().unwrap().apply(&hop.pass));

        let depth = path.len() + 1;
        let reachable_at_callee = reachables.last().unwrap();

        if let Some(decls) = callee.secrets_required() {
            for decl in decls {
                if !decl.required {
                    continue;
                }
                if reachable_at_callee.contains(&decl.name) {
                    continue;
                }

                let drop_idx = (0..reachables.len() - 1).find(|&k| {
                    reachables[k].contains(&decl.name) && !reachables[k + 1].contains(&decl.name)
                });
                let Some(drop_idx) = drop_idx else { continue };

                if depth == 1 {
                    let key = format!(
                        "Miss|{}|{}|{}|{}",
                        hop.caller_workflow, hop.caller_job, hop.callee_workflow, decl.name
                    );
                    if emitted.insert(key) {
                        findings.push(Finding {
                            kind: FindingKind::MissingSecretPropagation {
                                caller: hop.caller_workflow.clone(),
                                caller_job: hop.caller_job.clone(),
                                callee: hop.callee_workflow.clone(),
                                secret: decl.name.clone(),
                            },
                            severity: Severity::High,
                            location: FindingLocation::Job {
                                file: current.source.file.clone(),
                                workflow: hop.caller_workflow.clone(),
                                job: hop.caller_job.clone(),
                            },
                            message: format!(
                                "caller `{}` job `{}` does not pass required secret `{}` to callee `{}`",
                                hop.caller_workflow,
                                hop.caller_job,
                                decl.name,
                                hop.callee_workflow
                            ),
                        });
                    }
                } else {
                    let full_path: Vec<&Hop> = path.iter().chain(std::iter::once(&hop)).collect();
                    let drop_hop = full_path[drop_idx];
                    let dropped_at = drop_hop.caller_workflow.clone();
                    let entry = full_path[0].caller_workflow.clone();
                    let chain: Vec<String> = std::iter::once(entry.clone())
                        .chain(full_path.iter().map(|h| h.callee_workflow.clone()))
                        .collect();
                    let leaf = hop.callee_workflow.clone();
                    let dropped_at_file = workflow_by_id
                        .get(dropped_at.as_str())
                        .map(|w| w.source.file.clone())
                        .unwrap_or_else(|| PathBuf::from(&dropped_at));
                    let key = format!("Chain|{}|{}|{}|{}", entry, dropped_at, leaf, decl.name);
                    if emitted.insert(key) {
                        findings.push(Finding {
                            kind: FindingKind::SecretsInheritChainBreak {
                                entry: entry.clone(),
                                chain: chain.clone(),
                                dropped_at: dropped_at.clone(),
                                secret: decl.name.clone(),
                            },
                            severity: Severity::High,
                            location: FindingLocation::Job {
                                file: dropped_at_file,
                                workflow: dropped_at.clone(),
                                job: drop_hop.caller_job.clone(),
                            },
                            message: format!(
                                "secret `{}` required at `{}` is dropped at `{}` (job `{}`) in chain `{}` (depth={})",
                                decl.name,
                                leaf,
                                dropped_at,
                                drop_hop.caller_job,
                                chain.join(" -> "),
                                depth
                            ),
                        });
                    }
                }
            }
        }

        // Recurse into local callee, with cycle guard.
        if !visiting.contains(&callee.id.0) {
            visiting.insert(callee.id.0.clone());
            path.push(hop.clone());
            dfs(callee, workflow_by_id, path, visiting, findings, emitted);
            path.pop();
            visiting.remove(&callee.id.0);
        }
    }
}

fn location_key(loc: &FindingLocation) -> String {
    match loc {
        FindingLocation::Workflow { file } => format!("{}::", file.display()),
        FindingLocation::Job {
            file,
            workflow: _,
            job,
        } => format!("{}::{}", file.display(), job),
    }
}

fn kind_discriminator(k: &FindingKind) -> u8 {
    match k {
        FindingKind::MissingSecretPropagation { .. } => 0,
        FindingKind::SecretsInheritChainBreak { .. } => 1,
        FindingKind::EnvironmentInWorkflowCallCallee { .. } => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        CallsWorkflow, EventExtras, EventKind, Ir, Job, JobEnvironment, JobId, SecretDecl,
        SecretsPass, SourcePos, Step, TriggerSpec, Workflow, WorkflowId, WorkflowRef,
    };

    fn fake_finding(severity: Severity, workflow: &str) -> Finding {
        Finding {
            kind: FindingKind::EnvironmentInWorkflowCallCallee {
                workflow: workflow.into(),
                job: "build".into(),
                environment: "prod".into(),
            },
            severity,
            location: FindingLocation::Workflow {
                file: PathBuf::from(workflow),
            },
            message: String::new(),
        }
    }

    fn empty_job(id: &str, wf_id: &str) -> Job {
        Job {
            id: JobId(id.into()),
            workflow: WorkflowId(wf_id.into()),
            needs: vec![],
            permissions: None,
            steps: Vec::<Step>::new(),
            calls_workflow: None,
            runs_on: None,
            outputs: BTreeMap::new(),
            source: SourcePos {
                file: PathBuf::from(wf_id),
                line: None,
            },
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            if_expr: None,
            strategy: None,
            container: None,
            services: BTreeMap::new(),
            environment: None,
            annotations: Vec::new(),
        }
    }

    fn calling_job(id: &str, wf_id: &str, callee: WorkflowRef, secrets: SecretsPass) -> Job {
        let mut j = empty_job(id, wf_id);
        j.calls_workflow = Some(CallsWorkflow {
            workflow_ref: callee,
            with: BTreeMap::new(),
            secrets,
        });
        j
    }

    fn entry_workflow(id: &str, jobs: Vec<Job>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::from(id),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs,
            permissions: None,
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            annotations: Vec::new(),
        }
    }

    fn callable_workflow(id: &str, jobs: Vec<Job>, required_secrets: Vec<&str>) -> Workflow {
        let secrets: Vec<SecretDecl> = required_secrets
            .into_iter()
            .map(|n| SecretDecl {
                name: n.into(),
                required: true,
            })
            .collect();
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::from(id),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec {
                event: EventKind::WorkflowCall,
                branches: Default::default(),
                tags: Default::default(),
                paths: Default::default(),
                types: None,
                extras: Some(EventExtras::WorkflowCall {
                    inputs: vec![],
                    outputs: vec![],
                    secrets,
                }),
            }],
            jobs,
            permissions: None,
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            annotations: Vec::new(),
        }
    }

    fn ir_with(workflows: Vec<Workflow>) -> Ir {
        Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows,
            actions: Vec::new(),
            external_actions: Vec::new(),
        }
    }

    fn explicit_pass(pairs: &[(&str, &str)]) -> SecretsPass {
        let mut m = BTreeMap::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), (*v).to_string());
        }
        SecretsPass::Explicit(m)
    }

    #[test]
    fn severity_ordering_high_before_medium() {
        let mut findings = [
            fake_finding(Severity::Medium, "alpha.yml"),
            fake_finding(Severity::High, "zeta.yml"),
            fake_finding(Severity::Medium, "beta.yml"),
            fake_finding(Severity::High, "alpha.yml"),
        ];
        findings.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| location_key(&a.location).cmp(&location_key(&b.location)))
        });
        let order: Vec<(Severity, String)> = findings
            .iter()
            .map(|f| {
                let key = match &f.location {
                    FindingLocation::Workflow { file } => file.display().to_string(),
                    FindingLocation::Job { file, .. } => file.display().to_string(),
                };
                (f.severity, key)
            })
            .collect();
        assert_eq!(
            order.as_slice(),
            [
                (Severity::High, "alpha.yml".into()),
                (Severity::High, "zeta.yml".into()),
                (Severity::Medium, "alpha.yml".into()),
                (Severity::Medium, "beta.yml".into()),
            ]
        );
    }

    /// Inherit-chain depth ≥ 3 with a leaf that requires a secret. Every hop
    /// uses `secrets: inherit`, so the secret reaches the leaf and NO chain
    /// break is reported.
    #[test]
    fn inherit_chain_depth_three_clean_no_findings() {
        // entry → mid1 (inherit) → mid2 (inherit) → leaf (inherit)
        let entry = entry_workflow(
            ".github/workflows/entry.yml",
            vec![calling_job(
                "go",
                ".github/workflows/entry.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/mid1.yml".into())),
                SecretsPass::Inherit,
            )],
        );
        let mid1 = callable_workflow(
            ".github/workflows/mid1.yml",
            vec![calling_job(
                "go",
                ".github/workflows/mid1.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/mid2.yml".into())),
                SecretsPass::Inherit,
            )],
            vec![],
        );
        let mid2 = callable_workflow(
            ".github/workflows/mid2.yml",
            vec![calling_job(
                "go",
                ".github/workflows/mid2.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/leaf.yml".into())),
                SecretsPass::Inherit,
            )],
            vec![],
        );
        let leaf = callable_workflow(
            ".github/workflows/leaf.yml",
            vec![empty_job("done", ".github/workflows/leaf.yml")],
            vec!["NPM_TOKEN"],
        );

        let findings = check(&ir_with(vec![entry, mid1, mid2, leaf]));
        let breaks: Vec<&Finding> = findings
            .iter()
            .filter(|f| matches!(&f.kind, FindingKind::SecretsInheritChainBreak { .. }))
            .collect();
        assert!(
            breaks.is_empty(),
            "all-inherit chain depth=3 must not break: {findings:#?}"
        );
    }

    /// Inherit-chain depth ≥ 3 where a middle hop drops to `secrets: None`:
    /// emit `SecretsInheritChainBreak` reporting the dropping workflow.
    #[test]
    fn inherit_chain_break_at_middle_hop_emits_finding() {
        let entry = entry_workflow(
            ".github/workflows/entry.yml",
            vec![calling_job(
                "go",
                ".github/workflows/entry.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/mid1.yml".into())),
                SecretsPass::Inherit,
            )],
        );
        // mid1 drops the secret by declaring no secrets passing.
        let mid1 = callable_workflow(
            ".github/workflows/mid1.yml",
            vec![calling_job(
                "go",
                ".github/workflows/mid1.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/mid2.yml".into())),
                SecretsPass::None,
            )],
            vec![],
        );
        let mid2 = callable_workflow(
            ".github/workflows/mid2.yml",
            vec![calling_job(
                "go",
                ".github/workflows/mid2.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/leaf.yml".into())),
                SecretsPass::Inherit,
            )],
            vec![],
        );
        let leaf = callable_workflow(
            ".github/workflows/leaf.yml",
            vec![empty_job("done", ".github/workflows/leaf.yml")],
            vec!["NPM_TOKEN"],
        );

        let findings = check(&ir_with(vec![entry, mid1, mid2, leaf]));
        let break_finding = findings
            .iter()
            .find(|f| matches!(&f.kind, FindingKind::SecretsInheritChainBreak { .. }))
            .expect("expected chain break");
        match &break_finding.kind {
            FindingKind::SecretsInheritChainBreak {
                entry,
                dropped_at,
                secret,
                chain,
            } => {
                assert_eq!(entry, ".github/workflows/entry.yml");
                assert_eq!(dropped_at, ".github/workflows/mid1.yml");
                assert_eq!(secret, "NPM_TOKEN");
                assert_eq!(
                    chain.len(),
                    4,
                    "chain should record all 4 workflows: {chain:?}"
                );
            }
            other => panic!("unexpected kind: {other:?}"),
        }
    }

    /// Explicit-map propagation with a rename: caller passes
    /// `NPM_TOKEN: ${{ secrets.SOURCE }}`. The callee requires `NPM_TOKEN` —
    /// the caller's key carries the secret under that name, so no finding.
    #[test]
    fn explicit_pass_with_rename_satisfies_required_secret() {
        let entry = entry_workflow(
            ".github/workflows/entry.yml",
            vec![calling_job(
                "publish",
                ".github/workflows/entry.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/release.yml".into())),
                explicit_pass(&[("NPM_TOKEN", "${{ secrets.SOURCE_TOKEN }}")]),
            )],
        );
        let callee = callable_workflow(
            ".github/workflows/release.yml",
            vec![empty_job("publish", ".github/workflows/release.yml")],
            vec!["NPM_TOKEN"],
        );
        let findings = check(&ir_with(vec![entry, callee]));
        let missing = findings
            .iter()
            .any(|f| matches!(&f.kind, FindingKind::MissingSecretPropagation { .. }));
        assert!(
            !missing,
            "explicit pass with rename satisfies required secret, got: {findings:#?}"
        );
    }

    /// Caller passes `secrets: {}` (explicit empty map). The callee declares
    /// `NPM_TOKEN: required: true`. Depth = 1 → MissingSecretPropagation.
    #[test]
    fn missing_secret_declaration_emits_high_finding() {
        let entry = entry_workflow(
            ".github/workflows/entry.yml",
            vec![calling_job(
                "publish",
                ".github/workflows/entry.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/release.yml".into())),
                explicit_pass(&[]),
            )],
        );
        let callee = callable_workflow(
            ".github/workflows/release.yml",
            vec![empty_job("publish", ".github/workflows/release.yml")],
            vec!["NPM_TOKEN"],
        );
        let findings = check(&ir_with(vec![entry, callee]));
        assert_eq!(findings.len(), 1, "expected one finding: {findings:#?}");
        assert_eq!(findings[0].severity, Severity::High);
        match &findings[0].kind {
            FindingKind::MissingSecretPropagation {
                caller,
                caller_job,
                callee,
                secret,
            } => {
                assert_eq!(caller, ".github/workflows/entry.yml");
                assert_eq!(caller_job, "publish");
                assert_eq!(callee, ".github/workflows/release.yml");
                assert_eq!(secret, "NPM_TOKEN");
            }
            other => panic!("unexpected kind: {other:?}"),
        }
    }

    /// Cross-repo (External) callee is opaque: we cannot read the callee's
    /// `secrets:` declarations, so no missing-secret finding is emitted.
    #[test]
    fn cross_repo_callee_is_opaque_no_missing_secret() {
        let entry = entry_workflow(
            ".github/workflows/entry.yml",
            vec![calling_job(
                "publish",
                ".github/workflows/entry.yml",
                WorkflowRef::External {
                    owner: "other-org".into(),
                    repo: "shared".into(),
                    path: ".github/workflows/release.yml".into(),
                    gitref: "v1".into(),
                },
                explicit_pass(&[]),
            )],
        );
        let findings = check(&ir_with(vec![entry]));
        assert!(
            findings.is_empty(),
            "external callee opaque; no findings expected: {findings:#?}"
        );
    }

    /// A reusable callee reached from an entry chain with a job-level
    /// `environment:` emits MEDIUM `EnvironmentInWorkflowCallCallee`
    /// (env-scoped secrets shadow caller-passed secrets per GHA spec).
    #[test]
    fn environment_in_callee_emits_medium_shadow_finding() {
        let entry = entry_workflow(
            ".github/workflows/entry.yml",
            vec![calling_job(
                "deploy",
                ".github/workflows/entry.yml",
                WorkflowRef::Local(WorkflowId(".github/workflows/release.yml".into())),
                SecretsPass::Inherit,
            )],
        );
        let mut callee_job = empty_job("publish", ".github/workflows/release.yml");
        callee_job.environment = Some(JobEnvironment {
            name: "production".into(),
            url: None,
        });
        let callee = callable_workflow(".github/workflows/release.yml", vec![callee_job], vec![]);
        let findings = check(&ir_with(vec![entry, callee]));
        let env_finding = findings
            .iter()
            .find(|f| matches!(&f.kind, FindingKind::EnvironmentInWorkflowCallCallee { .. }))
            .expect("expected env-shadow finding");
        assert_eq!(env_finding.severity, Severity::Medium);
        match &env_finding.kind {
            FindingKind::EnvironmentInWorkflowCallCallee {
                workflow,
                job,
                environment,
            } => {
                assert_eq!(workflow, ".github/workflows/release.yml");
                assert_eq!(job, "publish");
                assert_eq!(environment, "production");
            }
            other => panic!("unexpected kind: {other:?}"),
        }
    }

    /// Reachable::apply: `Inherit` from an `All` baseline preserves `All`;
    /// an explicit map after `All` narrows to the map's keys; intersecting
    /// further with a missing key drops it.
    #[test]
    fn reachable_apply_inherit_then_explicit_then_intersect() {
        let r0 = Reachable::All;
        let after_inherit = r0.apply(&SecretsPass::Inherit);
        assert!(matches!(after_inherit, Reachable::All));
        assert!(after_inherit.contains("ANYTHING"));

        let after_explicit = after_inherit.apply(&explicit_pass(&[("A", "x"), ("B", "y")]));
        assert!(after_explicit.contains("A"));
        assert!(after_explicit.contains("B"));
        assert!(!after_explicit.contains("C"));

        // A second explicit hop intersects: only "A" survives.
        let after_intersect = after_explicit.apply(&explicit_pass(&[("A", "x")]));
        assert!(after_intersect.contains("A"));
        assert!(!after_intersect.contains("B"));

        // None hop fully cuts the chain.
        let after_none = after_intersect.apply(&SecretsPass::None);
        assert!(!after_none.contains("A"));
    }
}
