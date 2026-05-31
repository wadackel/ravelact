//! Graph enrichment + priority derivation.
//!
//! Wraps a [`Finding`] + [`Attachment`] with the graph context ravelact alone
//! can provide — reachability, callers/callees, affected entry workflows,
//! orphan status, and the node's permission / secret posture — then derives a
//! `graph_priority` from that context.
//!
//! The original tool severity ([`Finding::severity`], "source severity") is
//! never mutated. `graph_priority` is a *separate* field, always accompanied by
//! [`PriorityReason`]s that justify every promotion / demotion.
//!
//! ## Priority rules
//!
//! Promotion (toward [`Severity::High`]) when the node is reachable from a
//! security-sensitive trigger or has sensitive write permissions:
//! - reachable from `pull_request_target` / `workflow_run` — these run with
//!   write tokens + secrets on untrusted input (privilege escalation).
//!   Ref: Events that trigger workflows —
//!   <https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows>
//! - `write-all`, or `contents: write` / `id-token: write` available.
//!   Ref: Permissions —
//!   <https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#permissions>
//!
//! Demotion (toward [`Severity::Low`]) when the finding is unlikely to matter:
//! - the node is an orphan (no entry workflow reaches it).
//! - the node lives under a test-fixtures path.
//!
//! Demotion wins over promotion (an orphan is not reachable anyway). Every
//! reason is recorded regardless of which way the value moved.

use serde::Serialize;

use crate::ir::{
    CoarseKind, Ir, JobId, Permissions, ScopeAccess, ScopeKey, SecretsPass, UsesRef, Workflow,
    WorkflowId, WorkflowRef,
};
use crate::query::callers::{callers, CallerHit};
use crate::query::impact::impact;
use crate::query::orphans::orphans;
use crate::query::walk::{for_each_outgoing_edge, Edge, Node};
use crate::query::Target;

use super::attach::{Attachment, NodeRef, SubAnchor};
use super::model::{Finding, Severity};

/// Triggers that run with write tokens + secrets on untrusted input, so a
/// finding reachable from one is a privilege-escalation risk. Shared with the
/// browse backend, which flags risky-entry workflows for dangerous-path edges.
pub(crate) const RISKY_TRIGGERS: &[&str] = &["pull_request_target", "workflow_run"];

/// Write scopes the priority heuristic treats as sensitive enough to promote.
/// Shared with the browse backend's per-node `has_write` aggregation.
pub(crate) const SENSITIVE_WRITE_SCOPES: &[&str] = &["contents", "id-token"];

/// Graph context for the node a finding attached to. Empty when the finding
/// could not be resolved to an IR node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct GraphContext {
    /// Entry-point trigger events that reach the node (via affected entry
    /// workflows). E.g. `["push"]`, `["pull_request_target"]`.
    pub reachable_from: Vec<String>,
    /// Node ids that directly call/use the node.
    pub callers: Vec<String>,
    /// Outgoing call/use targets of the node (local ids or external refs).
    pub callees: Vec<String>,
    /// Entry-point workflows that reach the node, including the node itself
    /// when it is an entry point.
    pub affected_entrypoints: Vec<String>,
    pub is_orphan: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_context: Option<PermissionContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_context: Option<SecretContext>,
}

/// The node's declared permission posture, read directly from the IR (workflow
/// level, overridden by the job level when the finding anchors to a job/step).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PermissionContext {
    pub write_all: bool,
    /// Scope keys granted `write` (e.g. `contents`, `id-token`), sorted.
    pub write_scopes: Vec<String>,
    /// Where the effective permissions were read from: `workflow`, `job:<id>`,
    /// or `none-declared`.
    pub source: String,
}

/// Minimal secret posture of a workflow node, from the IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretContext {
    /// The workflow declares required secrets on its `workflow_call` trigger.
    pub requires_secrets: bool,
    /// Some job forwards secrets via `secrets: inherit`.
    pub inherits_secrets: bool,
    /// Some job passes secrets explicitly to a called workflow.
    pub passes_secrets: bool,
}

/// One justification for the derived `graph_priority`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum PriorityReason {
    ReachableFromRiskyTrigger { trigger: String },
    WriteAllAvailable,
    SensitiveWriteScope { scope: String },
    Orphaned,
    UnderTestFixtures,
}

/// A finding plus its graph context and derived priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EnrichedFinding {
    pub finding: Finding,
    pub attachment: Attachment,
    pub graph_context: GraphContext,
    pub graph_priority: Severity,
    pub priority_reasons: Vec<PriorityReason>,
}

/// Enrich a finding against the IR graph and derive its priority.
pub fn enrich(ir: &Ir, finding: Finding, attachment: Attachment) -> EnrichedFinding {
    let graph_context = build_context(ir, &attachment);
    let signals = priority_signals(&finding, &attachment, &graph_context);
    let (graph_priority, priority_reasons) = derive_priority(finding.severity, &signals);
    EnrichedFinding {
        finding,
        attachment,
        graph_context,
        graph_priority,
        priority_reasons,
    }
}

fn build_context(ir: &Ir, attachment: &Attachment) -> GraphContext {
    match &attachment.node {
        NodeRef::Workflow { id } => {
            let wf = ir.workflows.iter().find(|w| &w.id == id);
            let affected_entrypoints = affected_entrypoints_for(ir, &id.0, wf);
            GraphContext {
                reachable_from: reachable_from(ir, &affected_entrypoints),
                callers: caller_ids(ir, &Target::Workflow(id.clone())),
                callees: wf
                    .map(|w| callee_ids(Node::Workflow(w)))
                    .unwrap_or_default(),
                affected_entrypoints,
                is_orphan: orphans(ir).unused_workflows.contains(id),
                permission_context: wf.and_then(|w| permission_context(w, &attachment.sub_anchor)),
                secret_context: wf.map(secret_context),
            }
        }
        NodeRef::Action { id } => {
            let act = ir.actions.iter().find(|a| &a.id == id);
            let affected_entrypoints: Vec<String> = impact(ir, std::slice::from_ref(&id.0))
                .0
                .workflows
                .into_iter()
                .map(|w| w.0)
                .collect();
            GraphContext {
                reachable_from: reachable_from(ir, &affected_entrypoints),
                callers: caller_ids(ir, &Target::Action(id.clone())),
                callees: act.map(|a| callee_ids(Node::Action(a))).unwrap_or_default(),
                affected_entrypoints,
                is_orphan: orphans(ir).unused_actions.iter().any(|(a, _)| a == id),
                permission_context: None,
                secret_context: None,
            }
        }
        NodeRef::Unresolved { .. } => GraphContext::default(),
    }
}

/// Entry workflows reaching the node: downstream callers from `impact`, plus
/// the node itself when it is an entry-point workflow (impact excludes the
/// seed, so a finding on an entry workflow would otherwise show none).
fn affected_entrypoints_for(ir: &Ir, node_path: &str, wf: Option<&Workflow>) -> Vec<String> {
    let mut entrypoints: Vec<String> = impact(ir, &[node_path.to_string()])
        .0
        .workflows
        .into_iter()
        .map(|w| w.0)
        .collect();
    if let Some(w) = wf {
        if w.triggers.iter().any(|t| t.is_entry_point())
            && !entrypoints.iter().any(|e| e == node_path)
        {
            entrypoints.push(node_path.to_string());
        }
    }
    entrypoints.sort();
    entrypoints.dedup();
    entrypoints
}

/// Union of entry-point trigger event names across the affected entry workflows.
fn reachable_from(ir: &Ir, affected_entrypoints: &[String]) -> Vec<String> {
    let mut events: Vec<String> = Vec::new();
    for ep in affected_entrypoints {
        if let Some(w) = ir.workflows.iter().find(|w| w.id.0 == *ep) {
            for t in &w.triggers {
                if t.is_entry_point() {
                    events.push(t.event.name().to_string());
                }
            }
        }
    }
    events.sort();
    events.dedup();
    events
}

fn caller_ids(ir: &Ir, target: &Target) -> Vec<String> {
    let mut ids: Vec<String> = callers(ir, target)
        .into_iter()
        .map(|hit| match hit {
            CallerHit::JobCall { workflow, .. } | CallerHit::Step { workflow, .. } => workflow.0,
            CallerHit::CompositeStep { action, .. } => action.0,
            CallerHit::Annotated { workflow, .. } => workflow.0,
            CallerHit::AnnotatedComposite { action, .. } => action.0,
        })
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn callee_ids(node: Node<'_>) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for_each_outgoing_edge(node, |ctx| match ctx.edge {
        Edge::CallsWorkflow(cw) => ids.push(workflow_ref_id(&cw.workflow_ref)),
        Edge::Uses(uses) => ids.push(uses_ref_id(uses)),
        Edge::Annotation(_) => {}
    });
    ids.sort();
    ids.dedup();
    ids
}

fn workflow_ref_id(wr: &WorkflowRef) -> String {
    match wr {
        WorkflowRef::Local(id) => id.0.clone(),
        WorkflowRef::External {
            owner,
            repo,
            gitref,
            ..
        } => format!("{owner}/{repo}@{gitref}"),
    }
}

fn uses_ref_id(uses: &UsesRef) -> String {
    match uses {
        UsesRef::LocalWorkflow(WorkflowId(id)) => id.clone(),
        UsesRef::LocalAction(id) => id.0.clone(),
        UsesRef::External {
            owner,
            repo,
            subpath,
            gitref,
            ..
        } => match subpath {
            Some(sub) => format!("{owner}/{repo}/{sub}@{gitref}"),
            None => format!("{owner}/{repo}@{gitref}"),
        },
        UsesRef::Docker(d) => format!("docker://{}", d.display_str()),
    }
}

/// Read the node's effective permissions: the job-level declaration when the
/// finding anchors to a job/step (per the GA spec, a job's `permissions:`
/// replaces the workflow's), otherwise the workflow level.
fn permission_context(wf: &Workflow, sub_anchor: &SubAnchor) -> Option<PermissionContext> {
    let job_id: Option<&JobId> = match sub_anchor {
        SubAnchor::Job { job } => Some(job),
        SubAnchor::Step { job: Some(job), .. } => Some(job),
        _ => None,
    };

    if let Some(jid) = job_id {
        if let Some(job) = wf.jobs.iter().find(|j| &j.id == jid) {
            if let Some(perms) = &job.permissions {
                let (write_all, write_scopes) = summarize_permissions(perms);
                return Some(PermissionContext {
                    write_all,
                    write_scopes,
                    source: format!("job:{}", jid.0),
                });
            }
        }
    }

    match &wf.permissions {
        Some(perms) => {
            let (write_all, write_scopes) = summarize_permissions(perms);
            Some(PermissionContext {
                write_all,
                write_scopes,
                source: "workflow".to_string(),
            })
        }
        None => Some(PermissionContext {
            write_all: false,
            write_scopes: Vec::new(),
            source: "none-declared".to_string(),
        }),
    }
}

/// `(write_all, sorted write scope keys)` for a permissions declaration.
fn summarize_permissions(perms: &Permissions) -> (bool, Vec<String>) {
    match perms {
        Permissions::Coarse(CoarseKind::WriteAll) => (true, Vec::new()),
        Permissions::Coarse(_) => (false, Vec::new()),
        Permissions::Scopes(map) => {
            let mut scopes: Vec<String> = map
                .iter()
                .filter(|(_, access)| matches!(access, ScopeAccess::Write))
                .map(|(key, _)| scope_key_label(key))
                .collect();
            scopes.sort();
            (false, scopes)
        }
    }
}

fn scope_key_label(key: &ScopeKey) -> String {
    match serde_json::to_value(key) {
        Ok(serde_json::Value::String(s)) => s,
        _ => format!("{key:?}"),
    }
}

fn secret_context(wf: &Workflow) -> SecretContext {
    let requires_secrets = wf
        .secrets_required()
        .map(|s| !s.is_empty())
        .unwrap_or(false);
    let mut inherits_secrets = false;
    let mut passes_secrets = false;
    for job in &wf.jobs {
        if let Some(call) = &job.calls_workflow {
            match &call.secrets {
                SecretsPass::Inherit => inherits_secrets = true,
                SecretsPass::Explicit(map) if !map.is_empty() => passes_secrets = true,
                _ => {}
            }
        }
    }
    SecretContext {
        requires_secrets,
        inherits_secrets,
        passes_secrets,
    }
}

/// The signals the priority heuristic consumes, distilled from the context.
struct PrioritySignals {
    risky_triggers: Vec<String>,
    has_write_all: bool,
    sensitive_write_scopes: Vec<String>,
    is_orphan: bool,
    under_test_fixtures: bool,
}

fn priority_signals(
    finding: &Finding,
    attachment: &Attachment,
    ctx: &GraphContext,
) -> PrioritySignals {
    let risky_triggers = ctx
        .reachable_from
        .iter()
        .filter(|t| is_risky_trigger(t))
        .cloned()
        .collect();
    let (has_write_all, sensitive_write_scopes) = match &ctx.permission_context {
        Some(pc) => (
            pc.write_all,
            pc.write_scopes
                .iter()
                .filter(|s| SENSITIVE_WRITE_SCOPES.contains(&s.as_str()))
                .cloned()
                .collect(),
        ),
        None => (false, Vec::new()),
    };
    PrioritySignals {
        risky_triggers,
        has_write_all,
        sensitive_write_scopes,
        is_orphan: ctx.is_orphan,
        under_test_fixtures: matches!(
            &attachment.node,
            NodeRef::Workflow { .. } | NodeRef::Action { .. }
        ) && under_test_fixtures(&finding.location.path.to_string_lossy()),
    }
}

fn is_risky_trigger(name: &str) -> bool {
    RISKY_TRIGGERS.contains(&name)
}

/// True when the node path sits under a test-fixtures directory.
fn under_test_fixtures(path: &str) -> bool {
    path.split('/').any(|seg| seg == "fixtures")
}

/// Derive `graph_priority` from the source severity and graph signals.
/// Demotion wins over promotion; every applicable reason is recorded.
fn derive_priority(source: Severity, signals: &PrioritySignals) -> (Severity, Vec<PriorityReason>) {
    let mut reasons = Vec::new();
    let mut promote = false;
    let mut demote = false;

    for t in &signals.risky_triggers {
        reasons.push(PriorityReason::ReachableFromRiskyTrigger { trigger: t.clone() });
        promote = true;
    }
    if signals.has_write_all {
        reasons.push(PriorityReason::WriteAllAvailable);
        promote = true;
    }
    for scope in &signals.sensitive_write_scopes {
        reasons.push(PriorityReason::SensitiveWriteScope {
            scope: scope.clone(),
        });
        promote = true;
    }
    if signals.is_orphan {
        reasons.push(PriorityReason::Orphaned);
        demote = true;
    }
    if signals.under_test_fixtures {
        reasons.push(PriorityReason::UnderTestFixtures);
        demote = true;
    }

    let priority = if demote {
        source.min(Severity::Low)
    } else if promote {
        source.max(Severity::High)
    } else {
        source
    };
    (priority, reasons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn signals() -> PrioritySignals {
        PrioritySignals {
            risky_triggers: Vec::new(),
            has_write_all: false,
            sensitive_write_scopes: Vec::new(),
            is_orphan: false,
            under_test_fixtures: false,
        }
    }

    #[test]
    fn no_signals_keeps_source_severity() {
        let (p, reasons) = derive_priority(Severity::Medium, &signals());
        assert_eq!(p, Severity::Medium);
        assert!(reasons.is_empty());
    }

    #[test]
    fn risky_trigger_promotes_to_high() {
        let mut s = signals();
        s.risky_triggers = vec!["pull_request_target".to_string()];
        let (p, reasons) = derive_priority(Severity::Medium, &s);
        assert_eq!(p, Severity::High);
        assert_eq!(
            reasons,
            vec![PriorityReason::ReachableFromRiskyTrigger {
                trigger: "pull_request_target".to_string()
            }]
        );
    }

    #[test]
    fn write_all_promotes() {
        let mut s = signals();
        s.has_write_all = true;
        let (p, reasons) = derive_priority(Severity::Low, &s);
        assert_eq!(p, Severity::High);
        assert!(reasons.contains(&PriorityReason::WriteAllAvailable));
    }

    #[test]
    fn sensitive_write_scope_promotes() {
        let mut s = signals();
        s.sensitive_write_scopes = vec!["id-token".to_string()];
        let (p, reasons) = derive_priority(Severity::Medium, &s);
        assert_eq!(p, Severity::High);
        assert!(reasons.contains(&PriorityReason::SensitiveWriteScope {
            scope: "id-token".to_string()
        }));
    }

    #[test]
    fn promotion_never_lowers_error() {
        // Error is the top tier; promotion must not reduce it to High.
        let mut s = signals();
        s.risky_triggers = vec!["workflow_run".to_string()];
        let (p, _) = derive_priority(Severity::Error, &s);
        assert_eq!(p, Severity::Error);
    }

    #[test]
    fn orphan_demotes_to_low() {
        let mut s = signals();
        s.is_orphan = true;
        let (p, reasons) = derive_priority(Severity::High, &s);
        assert_eq!(p, Severity::Low);
        assert!(reasons.contains(&PriorityReason::Orphaned));
    }

    #[test]
    fn test_fixture_demotes() {
        let mut s = signals();
        s.under_test_fixtures = true;
        let (p, reasons) = derive_priority(Severity::Error, &s);
        assert_eq!(p, Severity::Low);
        assert!(reasons.contains(&PriorityReason::UnderTestFixtures));
    }

    #[test]
    fn demotion_wins_over_promotion() {
        let mut s = signals();
        s.risky_triggers = vec!["pull_request_target".to_string()];
        s.is_orphan = true;
        let (p, reasons) = derive_priority(Severity::High, &s);
        assert_eq!(p, Severity::Low);
        // Both reasons recorded.
        assert!(reasons
            .iter()
            .any(|r| matches!(r, PriorityReason::ReachableFromRiskyTrigger { .. })));
        assert!(reasons.contains(&PriorityReason::Orphaned));
    }

    #[test]
    fn demotion_keeps_info_at_info() {
        let mut s = signals();
        s.is_orphan = true;
        let (p, _) = derive_priority(Severity::Info, &s);
        assert_eq!(p, Severity::Info);
    }

    #[test]
    fn risky_trigger_detection() {
        assert!(is_risky_trigger("pull_request_target"));
        assert!(is_risky_trigger("workflow_run"));
        assert!(!is_risky_trigger("push"));
        assert!(!is_risky_trigger("workflow_dispatch"));
    }

    #[test]
    fn under_test_fixtures_detection() {
        assert!(under_test_fixtures(
            "tests/fixtures/x/.github/workflows/ci.yml"
        ));
        assert!(!under_test_fixtures(".github/workflows/ci.yml"));
    }

    #[test]
    fn summarize_write_all() {
        let (write_all, scopes) = summarize_permissions(&Permissions::Coarse(CoarseKind::WriteAll));
        assert!(write_all);
        assert!(scopes.is_empty());
    }

    #[test]
    fn summarize_read_all_is_not_write() {
        let (write_all, scopes) = summarize_permissions(&Permissions::Coarse(CoarseKind::ReadAll));
        assert!(!write_all);
        assert!(scopes.is_empty());
    }

    #[test]
    fn summarize_scopes_collects_write_keys() {
        let mut map = BTreeMap::new();
        map.insert(ScopeKey::Contents, ScopeAccess::Write);
        map.insert(ScopeKey::IdToken, ScopeAccess::Write);
        map.insert(ScopeKey::Issues, ScopeAccess::Read);
        let (write_all, scopes) = summarize_permissions(&Permissions::Scopes(map));
        assert!(!write_all);
        assert_eq!(scopes, vec!["contents".to_string(), "id-token".to_string()]);
    }

    // ---- build_context against a real IR ---------------------------------

    use crate::findings::attach::{Attachment, Confidence, NodeRef, SubAnchor};
    use crate::findings::model::{Finding, FindingId, FindingSource, Location};

    fn simple_ir() -> Ir {
        crate::ir::build::build_ir(
            std::path::Path::new("tests/fixtures/simple"),
            &globset::GlobSet::empty(),
        )
        .expect("simple fixture should build")
    }

    fn finding_at(path: &str) -> Finding {
        Finding {
            id: FindingId("x".into()),
            source: FindingSource::Zizmor,
            rule_id: "zizmor/x".into(),
            title: "t".into(),
            message: "m".into(),
            severity: Severity::Medium,
            location: Location {
                path: path.into(),
                start_line: Some(1),
                start_column: None,
                end_line: None,
                end_column: None,
            },
            tags: vec![],
        }
    }

    #[test]
    fn build_context_job_anchor_reads_job_perms_and_workflow_secrets() {
        // simple/ci.yml: job `test` has job-level permissions, and job
        // `call-build` uses a reusable workflow with `secrets: inherit`.
        let ir = simple_ir();
        let attachment = Attachment {
            node: NodeRef::Workflow {
                id: WorkflowId(".github/workflows/ci.yml".into()),
            },
            sub_anchor: SubAnchor::Job {
                job: JobId("test".into()),
            },
            confidence: Confidence::Exact,
            reason: "r".into(),
        };
        let enriched = enrich(&ir, finding_at(".github/workflows/ci.yml"), attachment);
        let ctx = &enriched.graph_context;
        let pc = ctx
            .permission_context
            .as_ref()
            .expect("workflow node carries permission context");
        assert_eq!(pc.source, "job:test", "job-level permissions are read");
        let sc = ctx
            .secret_context
            .as_ref()
            .expect("workflow node carries secret context");
        assert!(sc.inherits_secrets, "call-build uses secrets: inherit");
    }

    #[test]
    fn build_context_unresolved_node_is_default() {
        let ir = simple_ir();
        let attachment = Attachment {
            node: NodeRef::Unresolved {
                path: "nope.yml".into(),
            },
            sub_anchor: SubAnchor::WorkflowFile,
            confidence: Confidence::FileOnly,
            reason: "r".into(),
        };
        let enriched = enrich(&ir, finding_at("nope.yml"), attachment);
        assert_eq!(enriched.graph_context, GraphContext::default());
    }
}
