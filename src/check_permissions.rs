use crate::ir::{CoarseKind, Ir, Job, Permissions, ScopeAccess, ScopeKey, Workflow, WorkflowRef};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// Permissions scopes recognized by GitHub Actions, as listed in the
/// `permissions:` documentation. `metadata` is implicit-read and is not a
/// declarable key, so it is omitted.
///
/// GA spec: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#permissions
const KNOWN_SCOPES: &[ScopeKey] = &[
    ScopeKey::Actions,
    ScopeKey::ArtifactMetadata,
    ScopeKey::Attestations,
    ScopeKey::Checks,
    ScopeKey::Contents,
    ScopeKey::Deployments,
    ScopeKey::Discussions,
    ScopeKey::IdToken,
    ScopeKey::Issues,
    ScopeKey::Models,
    ScopeKey::Packages,
    ScopeKey::Pages,
    ScopeKey::PullRequests,
    ScopeKey::RepositoryProjects,
    ScopeKey::SecurityEvents,
    ScopeKey::Statuses,
    ScopeKey::VulnerabilityAlerts,
];

/// Per-scope permission level. Ordering: `None < Read < Write`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Level {
    None,
    Read,
    Write,
}

fn scope_access_to_level(a: &ScopeAccess) -> Level {
    match a {
        ScopeAccess::Read => Level::Read,
        ScopeAccess::Write => Level::Write,
        // Unknown access values are treated conservatively as `None` — the
        // analyzer makes no claim rather than risk false positives.
        ScopeAccess::None | ScopeAccess::Unknown(_) => Level::None,
    }
}

/// Returns the string label used in analyzer output for a known scope.
/// `Unknown` variants are not in `KNOWN_SCOPES` and are never passed here.
fn scope_key_label(key: &ScopeKey) -> &'static str {
    match key {
        ScopeKey::Actions => "actions",
        ScopeKey::ArtifactMetadata => "artifact-metadata",
        ScopeKey::Attestations => "attestations",
        ScopeKey::Checks => "checks",
        ScopeKey::Contents => "contents",
        ScopeKey::Deployments => "deployments",
        ScopeKey::Discussions => "discussions",
        ScopeKey::IdToken => "id-token",
        ScopeKey::Issues => "issues",
        ScopeKey::Models => "models",
        ScopeKey::Packages => "packages",
        ScopeKey::Pages => "pages",
        ScopeKey::PullRequests => "pull-requests",
        ScopeKey::RepositoryProjects => "repository-projects",
        ScopeKey::SecurityEvents => "security-events",
        ScopeKey::Statuses => "statuses",
        ScopeKey::VulnerabilityAlerts => "vulnerability-alerts",
        // Not reachable via KNOWN_SCOPES but must be exhaustive.
        ScopeKey::Unknown(s) => {
            // Safety: this arm is unreachable for KNOWN_SCOPES iteration.
            // Return a leak-free static placeholder. The caller never reaches
            // this arm in practice.
            let _ = s;
            "<unknown>"
        }
    }
}

/// Expand a `Permissions` declaration into a `BTreeMap<scope_label, Level>`
/// over the known scope set. `Coarse(ReadAll)` and `Coarse(WriteAll)` fan out
/// to every scope. `Scopes(map)` reads each declared scope's level and fills
/// the rest with `None` (per GHA spec: undeclared scopes inside a
/// `permissions:` block are denied). `Coarse(Unknown(_))` yields an empty map
/// — the analyzer makes no claim rather than risk false positives.
pub(crate) fn normalize(p: &Permissions) -> BTreeMap<&'static str, Level> {
    let mut out = BTreeMap::new();
    match p {
        Permissions::Coarse(kind) => {
            let level = match kind {
                CoarseKind::ReadAll => Some(Level::Read),
                CoarseKind::WriteAll => Some(Level::Write),
                CoarseKind::Unknown(_) => None,
            };
            if let Some(level) = level {
                for key in KNOWN_SCOPES {
                    out.insert(scope_key_label(key), level);
                }
            }
        }
        Permissions::Scopes(map) => {
            for key in KNOWN_SCOPES {
                let level = map
                    .get(key)
                    .map(scope_access_to_level)
                    .unwrap_or(Level::None);
                out.insert(scope_key_label(key), level);
            }
        }
    }
    out
}

/// Apply GHA's "job overrides workflow" rule. When a job declares its own
/// `permissions:`, the workflow-level value is ignored entirely; otherwise
/// the workflow-level value (if any) applies. `None` means neither layer
/// declared anything, which falls through to the repository default —
/// the analyzer cannot know that value, so it returns `None` and lets the
/// caller decide how to handle the unknown.
pub(crate) fn job_effective(
    workflow_perms: Option<&Permissions>,
    job_perms: Option<&Permissions>,
) -> Option<BTreeMap<&'static str, Level>> {
    if let Some(j) = job_perms {
        Some(normalize(j))
    } else {
        workflow_perms.map(normalize)
    }
}

pub(crate) fn is_coarse_write_all(p: &Permissions) -> bool {
    matches!(p, Permissions::Coarse(CoarseKind::WriteAll))
}

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

/// One hop along the caller chain that ends in an escalating callee.
/// `workflow` is the file id of the workflow declaring the calling job;
/// `job` is the calling job id within that workflow. The escalating callee
/// itself is NOT part of `chain` — it appears in `CalleeEscalatesCaller`'s
/// `callee` / `callee_job` / `location` fields.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChainStep {
    pub workflow: String,
    pub job: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum FindingKind {
    /// Coarse `permissions: write-all` declared on an entry-point workflow
    /// (workflow-level or any job-level). HIGH.
    OverlyBroadCoarse {
        workflow: String,
        job: Option<String>,
    },
    /// A reusable workflow callee declares permissions broader than the
    /// caller-chain's inherited cap for at least one scope. HIGH.
    ///
    /// `chain` records the path from the entry-point job through each
    /// caller-side hop down to (and including) the hop that calls the
    /// escalating callee. For a 1-hop direct caller→callee comparison,
    /// `chain` has length 1 (the entry-point job itself).
    CalleeEscalatesCaller {
        caller: String,
        caller_job: String,
        callee: String,
        callee_job: String,
        scopes: Vec<String>,
        chain: Vec<ChainStep>,
    },
    /// An entry-point workflow has no `permissions:` declared at the workflow
    /// level and at least one job also has no `permissions:`. The run
    /// resolves to the repository default `GITHUB_TOKEN` configuration,
    /// which on legacy repos is `write-all`. MEDIUM.
    ImplicitRepoDefault { workflow: String, jobs: Vec<String> },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "scope")]
pub enum FindingLocation {
    Workflow {
        file: PathBuf,
    },
    Job {
        file: PathBuf,
        workflow: String,
        job: String,
    },
}

impl Finding {
    fn kind_name(&self) -> &'static str {
        match &self.kind {
            FindingKind::OverlyBroadCoarse { .. } => "overly-broad-coarse",
            FindingKind::CalleeEscalatesCaller { .. } => "callee-escalates-caller",
            FindingKind::ImplicitRepoDefault { .. } => "implicit-repo-default",
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
            } => {
                format!("{}:{}", file.display(), job)
            }
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

pub fn check(ir: &Ir) -> Vec<Finding> {
    let mut out = Vec::new();

    let workflow_by_id: BTreeMap<&str, &Workflow> =
        ir.workflows.iter().map(|w| (w.id.0.as_str(), w)).collect();

    // (b) Transitive caller→callee chain — propagate min(cap, callee_wf,
    // callee_job) per scope from each entry-point job through every
    // `workflow_call` hop. Spec:
    // https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows#using-permissions
    let mut ctx = DfsCtx {
        workflow_by_id: &workflow_by_id,
        path: Vec::new(),
        visiting: BTreeSet::new(),
        findings: Vec::new(),
        emitted: BTreeMap::new(),
    };
    for entry_wf in &ir.workflows {
        if !entry_wf.triggers.iter().any(|t| t.is_entry_point()) {
            continue;
        }
        for entry_job in &entry_wf.jobs {
            // None cap means neither layer declared anything; the
            // `ImplicitRepoDefault` finding already surfaces that exposure,
            // so the chain walk skips it to avoid duplicate counting.
            let Some(cap) = job_effective(
                entry_wf.permissions.as_ref(),
                entry_job.permissions.as_ref(),
            ) else {
                continue;
            };
            ctx.seed(entry_wf, entry_job, &cap);
        }
    }
    out.extend(ctx.findings);

    for wf in &ir.workflows {
        let is_entry = wf.triggers.iter().any(|t| t.is_entry_point());
        if !is_entry {
            continue;
        }

        // (a) Coarse `write-all` declared at workflow level or any job level.
        if let Some(perms) = &wf.permissions {
            if is_coarse_write_all(perms) {
                out.push(Finding {
                    kind: FindingKind::OverlyBroadCoarse {
                        workflow: wf.id.0.clone(),
                        job: None,
                    },
                    severity: Severity::High,
                    location: FindingLocation::Workflow {
                        file: wf.source.file.clone(),
                    },
                    message: format!(
                        "entry-point workflow `{}` declares `permissions: write-all` at the workflow level",
                        wf.id.0
                    ),
                });
            }
        }
        for job in &wf.jobs {
            if let Some(perms) = &job.permissions {
                if is_coarse_write_all(perms) {
                    out.push(Finding {
                        kind: FindingKind::OverlyBroadCoarse {
                            workflow: wf.id.0.clone(),
                            job: Some(job.id.0.clone()),
                        },
                        severity: Severity::High,
                        location: FindingLocation::Job {
                            file: wf.source.file.clone(),
                            workflow: wf.id.0.clone(),
                            job: job.id.0.clone(),
                        },
                        message: format!(
                            "entry-point workflow `{}` job `{}` declares `permissions: write-all`",
                            wf.id.0, job.id.0
                        ),
                    });
                }
            }
        }

        // (c) Implicit repository default: workflow-level perms absent AND
        // at least one job has no perms key. `permissions: {}` (= explicit
        // empty Scopes) is treated as a deliberate declaration and does
        // NOT trigger this finding.
        if wf.permissions.is_none() {
            let jobs_without_perms: Vec<String> = wf
                .jobs
                .iter()
                .filter(|j| j.permissions.is_none())
                .map(|j| j.id.0.clone())
                .collect();
            if !jobs_without_perms.is_empty() {
                let job_list = jobs_without_perms.join(", ");
                out.push(Finding {
                    kind: FindingKind::ImplicitRepoDefault {
                        workflow: wf.id.0.clone(),
                        jobs: jobs_without_perms,
                    },
                    severity: Severity::Medium,
                    location: FindingLocation::Workflow {
                        file: wf.source.file.clone(),
                    },
                    message: format!(
                        "entry-point workflow `{}` declares no `permissions:` and jobs [{}] inherit the repository default",
                        wf.id.0, job_list
                    ),
                });
            }
        }
    }

    // High severity first; within a severity tier, deterministic by location
    // string so snapshots stay stable across reorderings of the IR walk.
    out.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then_with(|| location_key(&a.location).cmp(&location_key(&b.location)))
    });

    out
}

fn location_key(loc: &FindingLocation) -> String {
    match loc {
        FindingLocation::Workflow { file } => format!("{}::", file.display()),
        FindingLocation::Job {
            file,
            workflow: _,
            job,
        } => {
            format!("{}::{}", file.display(), job)
        }
    }
}

/// Per-scope `min(a, b)` over the known scope set. Any scope absent from a
/// map (the `Coarse(Unknown(_))` case, where `normalize` returns an empty
/// map) is treated as `Level::None` — the most conservative choice: an
/// unknown coarse declaration narrows the cap to `none` everywhere rather
/// than risk false negatives downstream. Same fallback applies to scopes
/// outside `KNOWN_SCOPES`.
fn min_per_scope(
    a: &BTreeMap<&'static str, Level>,
    b: &BTreeMap<&'static str, Level>,
) -> BTreeMap<&'static str, Level> {
    let mut out = BTreeMap::new();
    for scope in KNOWN_SCOPES {
        let label = scope_key_label(scope);
        let av = a.get(label).copied().unwrap_or(Level::None);
        let bv = b.get(label).copied().unwrap_or(Level::None);
        out.insert(label, av.min(bv));
    }
    out
}

/// Compute the cap that applies inside a callee job, given the cap entering
/// the call edge and the callee's workflow / job-level declarations.
/// Omitted layers inherit the inbound cap unchanged.
fn propagated_cap(
    cap: &BTreeMap<&'static str, Level>,
    wf_perms: Option<&Permissions>,
    job_perms: Option<&Permissions>,
) -> BTreeMap<&'static str, Level> {
    let after_wf = match wf_perms.map(normalize) {
        Some(d) => min_per_scope(cap, &d),
        None => cap.clone(),
    };
    match job_perms.map(normalize) {
        Some(d) => min_per_scope(&after_wf, &d),
        None => after_wf,
    }
}

/// Per-scope `decl > cap` check. Returns the labels (in `KNOWN_SCOPES`
/// iteration order) of every scope where `decl` exceeds `cap`.
fn scopes_exceeding(
    cap: &BTreeMap<&'static str, Level>,
    decl: &BTreeMap<&'static str, Level>,
) -> Vec<String> {
    let mut out = Vec::new();
    for scope in KNOWN_SCOPES {
        let label = scope_key_label(scope);
        let cap_lvl = cap.get(label).copied().unwrap_or(Level::None);
        let dec_lvl = decl.get(label).copied().unwrap_or(Level::None);
        if dec_lvl > cap_lvl {
            out.push(label.to_string());
        }
    }
    out
}

/// Dedup key for the transitive escalation walk. Two paths reaching the same
/// `(entry, leaf, scopes)` triple collapse to a single finding; we keep the
/// shortest chain for the smallest reproducer.
type DedupKey = (String, String, String, String, String);

/// Mutable state threaded through the transitive escalation walk. Bundling
/// the four accumulators keeps `descend` symmetric — `seed` is the only
/// outer entry and owns the entry-hop push/pop, insert/remove pairing.
/// Cycle protection is by the `visiting` set keyed on workflow id (matches
/// `check_secrets.rs`'s pattern).
struct DfsCtx<'a> {
    workflow_by_id: &'a BTreeMap<&'a str, &'a Workflow>,
    path: Vec<ChainStep>,
    visiting: BTreeSet<String>,
    findings: Vec<Finding>,
    emitted: BTreeMap<DedupKey, usize>,
}

impl<'a> DfsCtx<'a> {
    fn seed(&mut self, entry_wf: &Workflow, entry_job: &Job, cap: &BTreeMap<&'static str, Level>) {
        self.path.push(ChainStep {
            workflow: entry_wf.id.0.clone(),
            job: entry_job.id.0.clone(),
        });
        self.visiting.insert(entry_wf.id.0.clone());
        self.descend(entry_wf, entry_job, cap);
        self.visiting.remove(entry_wf.id.0.as_str());
        self.path.pop();
    }

    fn descend(
        &mut self,
        current_wf: &Workflow,
        current_job: &Job,
        cap: &BTreeMap<&'static str, Level>,
    ) {
        let Some(call) = &current_job.calls_workflow else {
            return;
        };
        let WorkflowRef::Local(callee_id) = &call.workflow_ref else {
            return;
        };
        if self.visiting.contains(callee_id.0.as_str()) {
            return;
        }
        let Some(callee) = self.workflow_by_id.get(callee_id.0.as_str()) else {
            return;
        };

        for callee_job in &callee.jobs {
            // A fully-omitted callee (`job_effective` returns `None`) inherits
            // the cap and cannot, by definition, exceed it — skip the check.
            if let Some(callee_decl) =
                job_effective(callee.permissions.as_ref(), callee_job.permissions.as_ref())
            {
                let escalated = scopes_exceeding(cap, &callee_decl);
                if !escalated.is_empty() {
                    self.record_escalation(current_wf, current_job, callee, callee_job, escalated);
                }
            }

            if callee_job.calls_workflow.is_some() {
                let cap_at_callee_job = propagated_cap(
                    cap,
                    callee.permissions.as_ref(),
                    callee_job.permissions.as_ref(),
                );
                self.visiting.insert(callee.id.0.clone());
                self.path.push(ChainStep {
                    workflow: callee.id.0.clone(),
                    job: callee_job.id.0.clone(),
                });
                self.descend(callee, callee_job, &cap_at_callee_job);
                self.path.pop();
                self.visiting.remove(callee.id.0.as_str());
            }
        }
    }

    fn record_escalation(
        &mut self,
        current_wf: &Workflow,
        current_job: &Job,
        callee: &Workflow,
        callee_job: &Job,
        scopes: Vec<String>,
    ) {
        let scopes_csv = scopes.join(", ");
        let entry = self
            .path
            .first()
            .expect("entry hop pushed before DFS start");
        let key: DedupKey = (
            entry.workflow.clone(),
            entry.job.clone(),
            callee.id.0.clone(),
            callee_job.id.0.clone(),
            scopes_csv.clone(),
        );
        let new_finding = Finding {
            kind: FindingKind::CalleeEscalatesCaller {
                caller: current_wf.id.0.clone(),
                caller_job: current_job.id.0.clone(),
                callee: callee.id.0.clone(),
                callee_job: callee_job.id.0.clone(),
                scopes,
                chain: self.path.clone(),
            },
            severity: Severity::High,
            location: FindingLocation::Job {
                file: callee.source.file.clone(),
                workflow: callee.id.0.clone(),
                job: callee_job.id.0.clone(),
            },
            message: format!(
                "callee `{}` job `{}` declares broader permissions than caller `{}` job `{}` for scope(s): {}",
                callee.id.0,
                callee_job.id.0,
                current_wf.id.0,
                current_job.id.0,
                scopes_csv
            ),
        };

        let new_chain_len = self.path.len();
        match self.emitted.get(&key).copied() {
            Some(existing_idx) => {
                let existing_len = match &self.findings[existing_idx].kind {
                    FindingKind::CalleeEscalatesCaller { chain, .. } => chain.len(),
                    _ => unreachable!("DedupKey only constructed for CalleeEscalatesCaller"),
                };
                if new_chain_len < existing_len {
                    self.findings[existing_idx] = new_finding;
                }
            }
            None => {
                let idx = self.findings.len();
                self.findings.push(new_finding);
                self.emitted.insert(key, idx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        CallsWorkflow, EventKind, Job, JobId, ScopeAccess, ScopeKey, SecretsPass, SourcePos, Step,
        TriggerSpec, Workflow, WorkflowId, WorkflowRef,
    };

    fn fake_finding(severity: Severity, workflow: &str) -> Finding {
        Finding {
            kind: FindingKind::ImplicitRepoDefault {
                workflow: workflow.into(),
                jobs: vec!["build".into()],
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

    fn calling_job(id: &str, wf_id: &str, callee: &str) -> Job {
        let mut j = empty_job(id, wf_id);
        j.calls_workflow = Some(CallsWorkflow {
            workflow_ref: WorkflowRef::Local(WorkflowId(callee.into())),
            with: BTreeMap::new(),
            secrets: SecretsPass::None,
        });
        j
    }

    fn entry_workflow(id: &str, jobs: Vec<Job>, perms: Option<Permissions>) -> Workflow {
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
            permissions: perms,
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            annotations: Vec::new(),
        }
    }

    fn callable_workflow(id: &str, jobs: Vec<Job>, perms: Option<Permissions>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::from(id),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            jobs,
            permissions: perms,
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            annotations: Vec::new(),
        }
    }

    fn scopes_map(entries: &[(ScopeKey, ScopeAccess)]) -> Permissions {
        let mut m = BTreeMap::new();
        for (k, v) in entries {
            m.insert(k.clone(), v.clone());
        }
        Permissions::Scopes(m)
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

    #[test]
    fn severity_ordering_high_before_medium() {
        let mut findings = [
            fake_finding(Severity::Medium, "alpha.yml"),
            fake_finding(Severity::High, "zeta.yml"),
            fake_finding(Severity::Medium, "beta.yml"),
            fake_finding(Severity::High, "alpha.yml"),
        ];
        // Reuse the same sort the real `check()` applies.
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

    /// Workflow-level `read-all` flows down to a job that omits `permissions:`.
    /// No coarse-write-all finding (Read != Write) and no implicit-repo-default
    /// (workflow-level perms exist).
    #[test]
    fn read_all_workflow_level_does_not_trigger_findings() {
        let wf = entry_workflow(
            ".github/workflows/a.yml",
            vec![empty_job("build", ".github/workflows/a.yml")],
            Some(Permissions::Coarse(CoarseKind::ReadAll)),
        );
        let findings = check(&ir_with(vec![wf]));
        assert!(
            findings.is_empty(),
            "read-all should not emit findings, got: {findings:#?}"
        );
    }

    /// `permissions: write-all` declared at the workflow level on an entry
    /// point fires `OverlyBroadCoarse` with `job: None` and HIGH severity.
    #[test]
    fn write_all_workflow_level_emits_overly_broad_coarse() {
        let wf = entry_workflow(
            ".github/workflows/a.yml",
            vec![empty_job("build", ".github/workflows/a.yml")],
            Some(Permissions::Coarse(CoarseKind::WriteAll)),
        );
        let findings = check(&ir_with(vec![wf]));
        let kinds: Vec<&FindingKind> = findings.iter().map(|f| &f.kind).collect();
        let has_wf_overbroad = kinds.iter().any(|k| {
            matches!(
                k,
                FindingKind::OverlyBroadCoarse {
                    workflow,
                    job: None,
                } if workflow == ".github/workflows/a.yml"
            )
        });
        assert!(
            has_wf_overbroad,
            "expected workflow-level OverlyBroadCoarse, got: {findings:#?}"
        );
        assert_eq!(findings[0].severity, Severity::High);
    }

    /// `permissions: write-all` declared at the job level (workflow declares
    /// minimal perms) fires `OverlyBroadCoarse` with `job: Some(_)`.
    #[test]
    fn write_all_job_level_emits_overly_broad_coarse_with_job() {
        let mut job = empty_job("escalate", ".github/workflows/a.yml");
        job.permissions = Some(Permissions::Coarse(CoarseKind::WriteAll));
        let wf = entry_workflow(
            ".github/workflows/a.yml",
            vec![job],
            // Workflow-level minimal so the workflow line cannot fire.
            Some(scopes_map(&[(ScopeKey::Contents, ScopeAccess::Read)])),
        );
        let findings = check(&ir_with(vec![wf]));
        let job_overbroad = findings.iter().any(|f| {
            matches!(
                &f.kind,
                FindingKind::OverlyBroadCoarse {
                    workflow,
                    job: Some(j),
                } if workflow == ".github/workflows/a.yml" && j == "escalate"
            )
        });
        assert!(
            job_overbroad,
            "expected job-level OverlyBroadCoarse, got: {findings:#?}"
        );
    }

    /// Empty `permissions: {}` (Scopes(empty)) at the workflow level is a
    /// deliberate "no permissions" declaration: no `OverlyBroadCoarse` fires
    /// AND `ImplicitRepoDefault` is suppressed even when jobs omit perms.
    #[test]
    fn empty_scopes_map_is_deliberate_drop_no_implicit_default() {
        let wf = entry_workflow(
            ".github/workflows/a.yml",
            vec![empty_job("build", ".github/workflows/a.yml")],
            Some(Permissions::Scopes(BTreeMap::new())),
        );
        let findings = check(&ir_with(vec![wf]));
        assert!(
            findings.is_empty(),
            "empty Scopes map at workflow level must suppress all findings, got: {findings:#?}"
        );
    }

    /// No `permissions:` at workflow OR job level on an entry-point workflow
    /// emits `ImplicitRepoDefault` with MEDIUM severity.
    #[test]
    fn implicit_repo_default_when_both_layers_omit_permissions() {
        let wf = entry_workflow(
            ".github/workflows/a.yml",
            vec![empty_job("build", ".github/workflows/a.yml")],
            None,
        );
        let findings = check(&ir_with(vec![wf]));
        assert_eq!(
            findings.len(),
            1,
            "expected exactly 1 finding: {findings:#?}"
        );
        assert_eq!(findings[0].severity, Severity::Medium);
        match &findings[0].kind {
            FindingKind::ImplicitRepoDefault { workflow, jobs } => {
                assert_eq!(workflow, ".github/workflows/a.yml");
                assert_eq!(jobs, &vec!["build".to_string()]);
            }
            other => panic!("expected ImplicitRepoDefault, got {other:?}"),
        }
    }

    /// A reusable callee declares broader permissions than its caller's cap;
    /// emit `CalleeEscalatesCaller` HIGH and record the calling chain.
    #[test]
    fn reusable_callee_escalates_caller_emits_finding() {
        // Caller: contents=read at the entry job level.
        // Callee (reusable): contents=write — escalates.
        let mut caller_job = calling_job(
            "call",
            ".github/workflows/entry.yml",
            ".github/workflows/reusable.yml",
        );
        caller_job.permissions = Some(scopes_map(&[(ScopeKey::Contents, ScopeAccess::Read)]));
        let entry = entry_workflow(".github/workflows/entry.yml", vec![caller_job], None);

        let mut callee_job = empty_job("inner", ".github/workflows/reusable.yml");
        callee_job.permissions = Some(scopes_map(&[(ScopeKey::Contents, ScopeAccess::Write)]));
        let callee = callable_workflow(".github/workflows/reusable.yml", vec![callee_job], None);

        let findings = check(&ir_with(vec![entry, callee]));
        let escalations: Vec<&Finding> = findings
            .iter()
            .filter(|f| matches!(&f.kind, FindingKind::CalleeEscalatesCaller { .. }))
            .collect();
        assert_eq!(
            escalations.len(),
            1,
            "expected one escalation finding, got: {findings:#?}"
        );
        match &escalations[0].kind {
            FindingKind::CalleeEscalatesCaller {
                caller,
                caller_job,
                callee,
                callee_job,
                scopes,
                chain,
            } => {
                assert_eq!(caller, ".github/workflows/entry.yml");
                assert_eq!(caller_job, "call");
                assert_eq!(callee, ".github/workflows/reusable.yml");
                assert_eq!(callee_job, "inner");
                assert_eq!(scopes, &vec!["contents".to_string()]);
                assert_eq!(chain.len(), 1, "1-hop chain has length 1, got {chain:?}");
            }
            other => panic!("expected CalleeEscalatesCaller, got {other:?}"),
        }
    }

    /// A reusable callee that omits both layers (`job_effective` returns None)
    /// inherits the caller's cap and produces NO escalation finding —
    /// the callee cannot exceed a cap it inherits.
    #[test]
    fn reusable_callee_full_omission_inherits_cap_no_escalation() {
        let mut caller_job = calling_job(
            "call",
            ".github/workflows/entry.yml",
            ".github/workflows/reusable.yml",
        );
        caller_job.permissions = Some(scopes_map(&[(ScopeKey::Contents, ScopeAccess::Read)]));
        let entry = entry_workflow(".github/workflows/entry.yml", vec![caller_job], None);

        // Callee declares no perms anywhere → fully omitted, inherits cap.
        let callee_job = empty_job("inner", ".github/workflows/reusable.yml");
        let callee = callable_workflow(".github/workflows/reusable.yml", vec![callee_job], None);

        let findings = check(&ir_with(vec![entry, callee]));
        let has_escalation = findings
            .iter()
            .any(|f| matches!(&f.kind, FindingKind::CalleeEscalatesCaller { .. }));
        assert!(
            !has_escalation,
            "fully-omitted callee inherits cap, must not emit escalation: {findings:#?}"
        );
    }

    /// Cross-repo (External) callee is opaque: the analyzer cannot read the
    /// callee's permissions, so no escalation finding fires for that hop.
    #[test]
    fn cross_repo_callee_is_opaque_no_escalation() {
        let mut caller_job = empty_job("call", ".github/workflows/entry.yml");
        caller_job.permissions = Some(scopes_map(&[(ScopeKey::Contents, ScopeAccess::Read)]));
        // Reference an external workflow — the IR contains no body for it.
        caller_job.calls_workflow = Some(CallsWorkflow {
            workflow_ref: WorkflowRef::External {
                owner: "other-org".into(),
                repo: "shared-flow".into(),
                path: ".github/workflows/build.yml".into(),
                gitref: "v1".into(),
            },
            with: BTreeMap::new(),
            secrets: SecretsPass::None,
        });
        let entry = entry_workflow(".github/workflows/entry.yml", vec![caller_job], None);

        let findings = check(&ir_with(vec![entry]));
        let has_escalation = findings
            .iter()
            .any(|f| matches!(&f.kind, FindingKind::CalleeEscalatesCaller { .. }));
        assert!(
            !has_escalation,
            "external callee body unavailable; no escalation may fire: {findings:#?}"
        );
    }

    /// Job-level perms override workflow-level perms entirely (GHA "job
    /// overrides workflow" rule). When the job declares Scopes that omit a
    /// scope, that scope is None — the workflow's value is NOT merged in.
    #[test]
    fn job_perms_replace_workflow_perms_completely() {
        let wf_perms = Permissions::Coarse(CoarseKind::WriteAll);
        let job_perms = scopes_map(&[(ScopeKey::Contents, ScopeAccess::Read)]);
        let effective = job_effective(Some(&wf_perms), Some(&job_perms)).unwrap();
        // Job's explicit Scopes wins; only `contents` is Read; `actions`
        // (declared by write-all at the workflow level) is dropped to None.
        assert_eq!(effective.get("contents").copied(), Some(Level::Read));
        assert_eq!(effective.get("actions").copied(), Some(Level::None));
        assert_eq!(effective.get("id-token").copied(), Some(Level::None));
    }
}
