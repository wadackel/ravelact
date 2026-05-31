//! Shared core for the external-finding overlay (M2).
//!
//! Loads SARIF findings, runs the M1 pipeline (`read_findings` → `attach` →
//! `enrich`), and provides the building blocks each command composes into its
//! own output: a node key for grouping, severity tallies (compact `H:2 M:1`
//! badges for the trace table fold and graph node labels), a terse finding
//! label for sections / inline markers, and a JSON projection.
//!
//! Scoping is the caller's job: every command already computed the set of
//! nodes it reports on (impacted / orphan / reachable / target+callers / all),
//! so it filters findings by membership of `node_key` in that set — no IR
//! re-walk, and `graph_context` stays as M1 computed it.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

use crate::findings::attach::{attach, NodeRef};
use crate::findings::enrich::{enrich, EnrichedFinding, PriorityReason};
use crate::findings::model::Severity;
use crate::findings::read_findings;
use crate::ir::{ActionId, Ir, WorkflowId};

/// Load every `--findings` file and run the full M1 pipeline, concatenating
/// the results (no cross-file dedup — see plan).
pub(in crate::cli) fn load_enriched(ir: &Ir, paths: &[PathBuf]) -> Result<Vec<EnrichedFinding>> {
    let mut out = Vec::new();
    for path in paths {
        for finding in read_findings(path)? {
            let attachment = attach(ir, &finding);
            out.push(enrich(ir, finding, attachment));
        }
    }
    Ok(out)
}

/// The IR node a finding resolved to, as a key commands can match against
/// their own result node ids. `Unresolved` findings have no key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::cli) enum NodeKey {
    Workflow(WorkflowId),
    Action(ActionId),
}

/// Key a finding by its attached node, or `None` when unresolved.
pub(in crate::cli) fn node_key(ef: &EnrichedFinding) -> Option<NodeKey> {
    match &ef.attachment.node {
        NodeRef::Workflow { id } => Some(NodeKey::Workflow(id.clone())),
        NodeRef::Action { id } => Some(NodeKey::Action(id.clone())),
        NodeRef::Unresolved { .. } => None,
    }
}

/// Group findings by their attached node (unresolved findings dropped). Within
/// each node, findings are sorted deterministically by (rule_id, line, column,
/// id) so output is independent of file / read order.
pub(in crate::cli) fn group_by_node(
    findings: &[EnrichedFinding],
) -> HashMap<NodeKey, Vec<&EnrichedFinding>> {
    let mut map: HashMap<NodeKey, Vec<&EnrichedFinding>> = HashMap::new();
    for ef in findings {
        if let Some(key) = node_key(ef) {
            map.entry(key).or_default().push(ef);
        }
    }
    for group in map.values_mut() {
        group.sort_by_key(|ef| sort_key(ef));
    }
    map
}

fn sort_key(ef: &EnrichedFinding) -> (String, u32, u32, String) {
    (
        ef.finding.rule_id.clone(),
        ef.finding.location.start_line.unwrap_or(0),
        ef.finding.location.start_column.unwrap_or(0),
        ef.finding.id.0.clone(),
    )
}

/// Which severity field drives counts / display: the tool's source severity,
/// or the ravelact-derived graph priority. Used by the trace table note fold
/// and the graph node-label counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::cli) enum Basis {
    Source,
    Priority,
}

impl Basis {
    pub(in crate::cli) fn from_show_priority(show_priority: bool) -> Basis {
        if show_priority {
            Basis::Priority
        } else {
            Basis::Source
        }
    }

    fn severity_of(self, ef: &EnrichedFinding) -> Severity {
        match self {
            Basis::Source => ef.finding.severity,
            Basis::Priority => ef.graph_priority,
        }
    }
}

/// Per-severity finding tally for a node, for compact `E:1 H:2 M:1` badges.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::cli) struct SeverityCounts {
    pub error: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub info: u32,
}

impl SeverityCounts {
    pub(in crate::cli) fn tally(findings: &[&EnrichedFinding], basis: Basis) -> SeverityCounts {
        let mut c = SeverityCounts::default();
        for ef in findings {
            match basis.severity_of(ef) {
                Severity::Error => c.error += 1,
                Severity::High => c.high += 1,
                Severity::Medium => c.medium += 1,
                Severity::Low => c.low += 1,
                Severity::Info => c.info += 1,
            }
        }
        c
    }

    /// Compact `E:1 H:2 M:1` form, only non-zero tiers, in severity order.
    /// Empty string when there are no findings.
    pub(in crate::cli) fn compact(&self) -> String {
        let mut parts = Vec::new();
        for (letter, n) in [
            ('E', self.error),
            ('H', self.high),
            ('M', self.medium),
            ('L', self.low),
            ('I', self.info),
        ] {
            if n > 0 {
                parts.push(format!("{letter}:{n}"));
            }
        }
        parts.join(" ")
    }
}

/// Lowercase severity word, matching the model's serde representation.
pub(in crate::cli) fn severity_word(sev: Severity) -> &'static str {
    match sev {
        Severity::Error => "error",
        Severity::High => "high",
        Severity::Medium => "medium",
        Severity::Low => "low",
        Severity::Info => "info",
    }
}

/// Rule id with the leading `<source>/` stripped for display
/// (`zizmor/unpinned-uses` → `unpinned-uses`).
fn rule_short(ef: &EnrichedFinding) -> &str {
    let prefix = format!("{}/", ef.finding.source.label());
    ef.finding
        .rule_id
        .strip_prefix(&prefix)
        .unwrap_or(&ef.finding.rule_id)
}

/// One-line label for a finding, reused by section bullets and trace markers.
/// Always shows the source severity; with `show_priority`, appends the graph
/// priority and its reasons.
pub(in crate::cli) fn finding_label(ef: &EnrichedFinding, show_priority: bool) -> String {
    let mut s = format!(
        "[{}/{}] {}",
        ef.finding.source.label(),
        severity_word(ef.finding.severity),
        rule_short(ef),
    );
    if let Some(line) = ef.finding.location.start_line {
        s.push_str(&format!(" (L{line})"));
    }
    if show_priority {
        s.push_str(&format!(" — graph: {}", severity_word(ef.graph_priority)));
        if !ef.priority_reasons.is_empty() {
            s.push_str(&format!(" [{}]", reasons_str(&ef.priority_reasons)));
        }
    }
    s
}

fn reasons_str(reasons: &[PriorityReason]) -> String {
    reasons
        .iter()
        .map(reason_word)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(in crate::cli) fn reason_word(reason: &PriorityReason) -> String {
    match reason {
        PriorityReason::ReachableFromRiskyTrigger { trigger } => {
            format!("reachable-from {trigger}")
        }
        PriorityReason::WriteAllAvailable => "write-all".to_string(),
        PriorityReason::SensitiveWriteScope { scope } => format!("write:{scope}"),
        PriorityReason::Orphaned => "orphaned".to_string(),
        PriorityReason::UnderTestFixtures => "under-test-fixtures".to_string(),
    }
}

/// Render a node's findings as indented bullet lines for text / markdown
/// section output. Returns an empty string when there are no findings.
pub(in crate::cli) fn render_node_findings(
    findings: &[&EnrichedFinding],
    show_priority: bool,
    indent: &str,
) -> String {
    let mut out = String::new();
    for ef in findings {
        out.push_str(indent);
        out.push_str("- ");
        out.push_str(&finding_label(ef, show_priority));
        out.push('\n');
    }
    out
}

/// The flat list of findings scoped to an ordered (possibly duplicated) set of
/// nodes — used by section-append commands to drive both the text body and the
/// JSON `findings` array consistently. Nodes are deduped (first occurrence
/// wins) and findings appear in node order, then sorted within each node.
pub(in crate::cli) fn scoped_findings<'a>(
    grouped: &HashMap<NodeKey, Vec<&'a EnrichedFinding>>,
    ordered_nodes: &[(NodeKey, String)],
) -> Vec<&'a EnrichedFinding> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (key, _) in ordered_nodes {
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(group) = grouped.get(key) {
            out.extend(group.iter().copied());
        }
    }
    out
}

/// Render a "findings per node" body for the given ordered node scope. Each
/// node with findings emits its display line followed by indented bullets;
/// nodes without findings are skipped. Returns "" when nothing to show. The
/// caller supplies its own section heading.
pub(in crate::cli) fn render_scoped_findings(
    grouped: &HashMap<NodeKey, Vec<&EnrichedFinding>>,
    ordered_nodes: &[(NodeKey, String)],
    show_priority: bool,
) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out = String::new();
    for (key, display) in ordered_nodes {
        if !seen.insert(key.clone()) {
            continue;
        }
        if let Some(group) = grouped.get(key) {
            out.push_str(&format!("  {display}\n"));
            out.push_str(&render_node_findings(group, show_priority, "    "));
        }
    }
    out
}

/// JSON projection: serialize the enriched findings as-is (source severity and
/// graph priority are both present; invariant to `--show-priority`).
pub(in crate::cli) fn findings_json(findings: &[&EnrichedFinding]) -> Result<serde_json::Value> {
    Ok(serde_json::to_value(findings)?)
}

/// Build the trace renderer's [`FindingMarks`](crate::query::trace_render::FindingMarks)
/// from grouped findings: per node, the `! …` tree marker lines plus the
/// compact severity-count note for the table fold.
pub(in crate::cli) fn trace_marks(
    grouped: &HashMap<NodeKey, Vec<&EnrichedFinding>>,
    show_priority: bool,
) -> crate::query::trace_render::FindingMarks {
    use crate::query::trace_render::{FindingMarks, NodeMarks};

    let basis = Basis::from_show_priority(show_priority);
    let mut marks = FindingMarks::default();
    for (key, group) in grouped {
        let node_marks = NodeMarks {
            lines: group
                .iter()
                .map(|ef| finding_label(ef, show_priority))
                .collect(),
            note: SeverityCounts::tally(group, basis).compact(),
        };
        match key {
            NodeKey::Workflow(id) => {
                marks.workflows.insert(id.0.clone(), node_marks);
            }
            NodeKey::Action(id) => {
                marks.actions.insert(id.0.clone(), node_marks);
            }
        }
    }
    marks
}

/// Build the `graph --highlight findings` overlay from grouped findings: per
/// node, a compact count badge and a severity-colored Mermaid `style` body.
/// Only resolved nodes are present (unresolved / `FileOnly` findings have no
/// node key and are therefore never styled, per the design).
pub(in crate::cli) fn graph_overlay(
    grouped: &HashMap<NodeKey, Vec<&EnrichedFinding>>,
    show_priority: bool,
) -> crate::query::mermaid::GraphOverlay {
    use crate::query::mermaid::{GraphNodeOverlay, GraphOverlay};

    let basis = Basis::from_show_priority(show_priority);
    let mut overlay = GraphOverlay::default();
    for (key, group) in grouped {
        let counts = SeverityCounts::tally(group, basis);
        let badge = counts.compact();
        if badge.is_empty() {
            continue;
        }
        let node = GraphNodeOverlay {
            badge,
            style: severity_style(&counts).to_string(),
        };
        match key {
            NodeKey::Workflow(id) => {
                overlay.workflows.insert(id.0.clone(), node);
            }
            NodeKey::Action(id) => {
                overlay.actions.insert(id.0.clone(), node);
            }
        }
    }
    overlay
}

/// Mermaid `style` body colored by the node's most severe finding tier.
fn severity_style(counts: &SeverityCounts) -> &'static str {
    if counts.error > 0 || counts.high > 0 {
        "fill:#f8d7da,stroke:#dc3545"
    } else if counts.medium > 0 {
        "fill:#fff3cd,stroke:#fd7e14"
    } else {
        "fill:#e2e3e5,stroke:#6c757d"
    }
}

/// The node scope of a trace result: every workflow / action node reachable in
/// the trace trees, in pre-order. Used to scope the JSON `findings` array.
pub(in crate::cli) fn trace_node_scope(
    entries: &[crate::query::trace::TraceEntry],
) -> Vec<(NodeKey, String)> {
    let mut scope = Vec::new();
    for entry in entries {
        collect_trace_nodes(&entry.root, &mut scope);
    }
    scope
}

fn collect_trace_nodes(node: &crate::query::trace::TraceNode, scope: &mut Vec<(NodeKey, String)>) {
    use crate::query::trace::TraceNode;
    match node {
        TraceNode::Workflow { id, children } => {
            scope.push((NodeKey::Workflow(id.clone()), id.0.clone()));
            for c in children {
                collect_trace_nodes(c, scope);
            }
        }
        TraceNode::Action { id, children } => {
            scope.push((NodeKey::Action(id.clone()), id.0.clone()));
            for c in children {
                collect_trace_nodes(c, scope);
            }
        }
        TraceNode::Annotated { children, .. } => {
            for c in children {
                collect_trace_nodes(c, scope);
            }
        }
        TraceNode::Guarded { inner, .. } => collect_trace_nodes(inner, scope),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::attach::{Attachment, Confidence, SubAnchor};
    use crate::findings::enrich::GraphContext;
    use crate::findings::model::{Finding, FindingId, FindingSource, Location};

    fn mk(
        rule_id: &str,
        source_sev: Severity,
        graph_prio: Severity,
        node: NodeRef,
        line: Option<u32>,
        reasons: Vec<PriorityReason>,
    ) -> EnrichedFinding {
        EnrichedFinding {
            finding: Finding {
                id: FindingId(format!("{rule_id}:{}", line.unwrap_or(0))),
                source: FindingSource::Zizmor,
                rule_id: rule_id.to_string(),
                title: "t".to_string(),
                message: "m".to_string(),
                severity: source_sev,
                location: Location {
                    path: "f.yml".into(),
                    start_line: line,
                    start_column: None,
                    end_line: None,
                    end_column: None,
                },
                tags: vec![],
            },
            attachment: Attachment {
                node,
                sub_anchor: SubAnchor::WorkflowFile,
                confidence: Confidence::Exact,
                reason: "r".to_string(),
            },
            graph_context: GraphContext::default(),
            graph_priority: graph_prio,
            priority_reasons: reasons,
        }
    }

    fn wf(id: &str) -> NodeRef {
        NodeRef::Workflow {
            id: WorkflowId(id.to_string()),
        }
    }

    #[test]
    fn node_key_maps_workflow_action_and_drops_unresolved() {
        let w = mk(
            "zizmor/x",
            Severity::High,
            Severity::High,
            wf("ci.yml"),
            None,
            vec![],
        );
        assert_eq!(
            node_key(&w),
            Some(NodeKey::Workflow(WorkflowId("ci.yml".into())))
        );
        let a = mk(
            "zizmor/x",
            Severity::High,
            Severity::High,
            NodeRef::Action {
                id: ActionId(".github/actions/a".into()),
            },
            None,
            vec![],
        );
        assert_eq!(
            node_key(&a),
            Some(NodeKey::Action(ActionId(".github/actions/a".into())))
        );
        let u = mk(
            "zizmor/x",
            Severity::High,
            Severity::High,
            NodeRef::Unresolved { path: "x".into() },
            None,
            vec![],
        );
        assert_eq!(node_key(&u), None);
    }

    #[test]
    fn group_by_node_buckets_and_sorts() {
        let findings = vec![
            mk(
                "zizmor/unpinned-uses",
                Severity::High,
                Severity::High,
                wf("ci.yml"),
                Some(16),
                vec![],
            ),
            mk(
                "zizmor/artipacked",
                Severity::Medium,
                Severity::High,
                wf("ci.yml"),
                Some(11),
                vec![],
            ),
            mk(
                "zizmor/x",
                Severity::High,
                Severity::Low,
                NodeRef::Action {
                    id: ActionId("a".into()),
                },
                Some(7),
                vec![],
            ),
        ];
        let grouped = group_by_node(&findings);
        let ci = grouped
            .get(&NodeKey::Workflow(WorkflowId("ci.yml".into())))
            .unwrap();
        // sorted by rule_id: artipacked before unpinned-uses
        assert_eq!(ci[0].finding.rule_id, "zizmor/artipacked");
        assert_eq!(ci[1].finding.rule_id, "zizmor/unpinned-uses");
        assert_eq!(
            grouped
                .get(&NodeKey::Action(ActionId("a".into())))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn severity_counts_compact_source_vs_priority() {
        let findings = [
            mk(
                "r1",
                Severity::Medium,
                Severity::High,
                wf("ci.yml"),
                Some(1),
                vec![],
            ),
            mk(
                "r2",
                Severity::High,
                Severity::High,
                wf("ci.yml"),
                Some(2),
                vec![],
            ),
        ];
        let refs: Vec<&EnrichedFinding> = findings.iter().collect();
        assert_eq!(
            SeverityCounts::tally(&refs, Basis::Source).compact(),
            "H:1 M:1"
        );
        assert_eq!(
            SeverityCounts::tally(&refs, Basis::Priority).compact(),
            "H:2"
        );
    }

    #[test]
    fn severity_counts_empty_compact() {
        assert_eq!(SeverityCounts::default().compact(), "");
    }

    #[test]
    fn basis_from_show_priority() {
        assert_eq!(Basis::from_show_priority(true), Basis::Priority);
        assert_eq!(Basis::from_show_priority(false), Basis::Source);
    }

    #[test]
    fn finding_label_source_only() {
        let f = mk(
            "zizmor/unpinned-uses",
            Severity::High,
            Severity::Low,
            wf("ci.yml"),
            Some(12),
            vec![],
        );
        assert_eq!(
            finding_label(&f, false),
            "[zizmor/high] unpinned-uses (L12)"
        );
    }

    #[test]
    fn finding_label_with_priority_and_reasons() {
        let f = mk(
            "zizmor/unpinned-uses",
            Severity::Medium,
            Severity::High,
            wf("ci.yml"),
            Some(12),
            vec![
                PriorityReason::ReachableFromRiskyTrigger {
                    trigger: "pull_request_target".into(),
                },
                PriorityReason::WriteAllAvailable,
            ],
        );
        assert_eq!(
            finding_label(&f, true),
            "[zizmor/medium] unpinned-uses (L12) — graph: high [reachable-from pull_request_target, write-all]"
        );
    }

    #[test]
    fn render_node_findings_indents_bullets() {
        let findings = [mk(
            "zizmor/x",
            Severity::High,
            Severity::High,
            wf("ci.yml"),
            Some(3),
            vec![],
        )];
        let refs: Vec<&EnrichedFinding> = findings.iter().collect();
        assert_eq!(
            render_node_findings(&refs, false, "    "),
            "    - [zizmor/high] x (L3)\n"
        );
    }

    #[test]
    fn findings_json_serializes_enriched() {
        let findings = [mk(
            "zizmor/x",
            Severity::High,
            Severity::High,
            wf("ci.yml"),
            Some(3),
            vec![],
        )];
        let refs: Vec<&EnrichedFinding> = findings.iter().collect();
        let v = findings_json(&refs).unwrap();
        assert_eq!(v[0]["finding"]["rule_id"], "zizmor/x");
        assert_eq!(v[0]["finding"]["severity"], "high");
        assert_eq!(v[0]["graph_priority"], "high");
    }

    #[test]
    fn render_scoped_findings_orders_dedups_and_skips_empty() {
        let findings = vec![
            mk(
                "zizmor/b",
                Severity::High,
                Severity::High,
                wf("ci.yml"),
                Some(5),
                vec![],
            ),
            mk(
                "zizmor/a",
                Severity::Medium,
                Severity::High,
                wf("ci.yml"),
                Some(2),
                vec![],
            ),
        ];
        let grouped = group_by_node(&findings);
        let ordered = vec![
            (
                NodeKey::Workflow(WorkflowId("ci.yml".into())),
                "ci.yml".to_string(),
            ),
            // duplicate node ignored
            (
                NodeKey::Workflow(WorkflowId("ci.yml".into())),
                "ci.yml".to_string(),
            ),
            // node with no findings skipped
            (
                NodeKey::Workflow(WorkflowId("other.yml".into())),
                "other.yml".to_string(),
            ),
        ];
        let body = render_scoped_findings(&grouped, &ordered, false);
        assert_eq!(
            body,
            "  ci.yml\n    - [zizmor/medium] a (L2)\n    - [zizmor/high] b (L5)\n"
        );
        // scoped_findings flattens in node order, deduped
        let flat = scoped_findings(&grouped, &ordered);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].finding.rule_id, "zizmor/a");
    }

    #[test]
    fn load_enriched_reads_fixture_and_resolves_nodes() {
        // End-to-end: build IR from the zizmor-findings fixture, load + enrich
        // its committed SARIF, and confirm findings resolve to IR nodes.
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/synthetic/zizmor-findings");
        let ir = crate::ir::build::build_ir(&fixture, &globset::GlobSet::empty()).unwrap();
        let enriched = load_enriched(&ir, &[fixture.join("zizmor.sarif")]).unwrap();
        assert!(!enriched.is_empty());
        let grouped = group_by_node(&enriched);
        assert!(grouped.contains_key(&NodeKey::Workflow(WorkflowId(
            ".github/workflows/ci.yml".into()
        ))));
    }
}
