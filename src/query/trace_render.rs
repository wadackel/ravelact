use std::collections::HashMap;

use crate::ir::{AnnotationVerb, ExternalActionRef};
use crate::markdown;
use crate::query::trace::{CycleTarget, TraceEntry, TraceNode};
use crate::ui::{KindTag, Ui};
use unicode_width::UnicodeWidthStr;

/// External-finding overlay data for the trace renderers, keyed by node id.
///
/// Built by `cmd_trace` from the M1 pipeline. The renderer consumes only plain
/// strings (pre-rendered labels + a compact count) so the query layer stays
/// decoupled from the CLI overlay types. Nodes absent from the maps render
/// exactly as before.
#[derive(Debug, Default)]
pub struct FindingMarks {
    pub workflows: HashMap<String, NodeMarks>,
    pub actions: HashMap<String, NodeMarks>,
}

/// Per-node overlay strings: `lines` are tree marker labels (each rendered as a
/// `! …` sub-line under the node); `note` is the compact severity count
/// (`H:2 M:1`) folded into the table's note column.
#[derive(Debug, Default, Clone)]
pub struct NodeMarks {
    pub lines: Vec<String>,
    pub note: String,
}

impl FindingMarks {
    fn marks_for(&self, node: &TraceNode) -> Option<&NodeMarks> {
        match node {
            TraceNode::Workflow { id, .. } => self.workflows.get(&id.0),
            TraceNode::Action { id, .. } => self.actions.get(&id.0),
            _ => None,
        }
    }

    fn lines_for(&self, node: &TraceNode) -> &[String] {
        self.marks_for(node)
            .map(|m| m.lines.as_slice())
            .unwrap_or(&[])
    }
}

/// Style flags for `render_tree`.
#[derive(Debug, Clone)]
pub struct TreeStyle {
    /// `true` → Unicode rounded box-drawing borders (`╭─`, `├─→`, `╰─→`, `│`).
    /// `false` → ASCII fallback (`+-`, `|->`, `\->`, `|`).
    pub unicode: bool,
}

/// Optional event-grouping header injected as the synthetic root of the tree.
/// When `cmd_trace` runs in tree format, the trigger event is hoisted from
/// the status header into the tree itself so each entry workflow visibly hangs
/// off the event that triggered it. Pass `None` to render roots side by side
/// (used by unit tests that exercise the connector mechanics in isolation).
/// The single lifetime is intentional: the event and summary are borrowed for
/// the same render call at every current call site.
pub struct EventMeta<'a> {
    pub event: &'a str,
    pub summary: &'a [String],
}

/// Render a list of trace trees as an indented tree.
///
/// With `event_meta = Some(_)`: emit `╭─ <event>   (<s[0]>, <s[1]>, ...)` as
/// the synthetic root and treat each `roots[i]` as a child sibling. Empty
/// `summary` omits the parens entirely.
/// With `event_meta = None`: each root is rendered side by side (separated by
/// a blank line). This mode is used by tests that exercise the connector
/// mechanics directly without an event group.
///
/// Spacer policy: only top-level workflow siblings (the children of the event
/// root, or independent roots when `event_meta = None`) get a 1-line spacer
/// between them. Per-node parent → child spacers were dropped to tighten
/// related items visually — internal density is the win.
pub fn render_tree(
    entries: &[TraceEntry],
    event_meta: Option<EventMeta<'_>>,
    style: &TreeStyle,
    ui: &Ui,
) -> String {
    render_tree_impl(entries, event_meta, style, ui, None)
}

/// `render_tree` with an external-finding overlay: each workflow / action node
/// gains `! …` finding sub-lines from `marks`.
pub fn render_tree_with_findings(
    entries: &[TraceEntry],
    event_meta: Option<EventMeta<'_>>,
    style: &TreeStyle,
    ui: &Ui,
    marks: &FindingMarks,
) -> String {
    render_tree_impl(entries, event_meta, style, ui, Some(marks))
}

fn render_tree_impl(
    entries: &[TraceEntry],
    event_meta: Option<EventMeta<'_>>,
    style: &TreeStyle,
    ui: &Ui,
    marks: Option<&FindingMarks>,
) -> String {
    let mut out = String::new();
    let mut chain: Vec<bool> = Vec::new();

    if let Some(em) = event_meta {
        // Synthetic event root: `╭─ <event>   (<s[0]>, <s[1]>, ...)`.
        let connector = if style.unicode { "╭─ " } else { "+- " };
        out.push_str(&ui.muted(connector));
        out.push_str(&ui.strong(em.event));
        if !em.summary.is_empty() {
            out.push_str("   ");
            let body = format!("({})", em.summary.join(", "));
            out.push_str(&ui.muted(&body));
        }
        out.push('\n');

        let n = entries.len();
        let stub = if style.unicode { "│" } else { "|" };
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                // Top-level workflow spacer: column-0 vertical guide between
                // depth=1 siblings so each entry workflow has breathing room
                // without internal density loss.
                out.push_str(&ui.muted(stub));
                out.push('\n');
            }
            chain.push(i == n - 1);
            render_entry_tree(entry, &mut chain, style, ui, &mut out, marks);
            chain.pop();
        }
    } else {
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            render_entry_tree(entry, &mut chain, style, ui, &mut out, marks);
        }
    }

    out
}

/// Render a single entry. The root (always `TraceNode::Workflow`) renders
/// like any other node, but a [`TriggerMatch`](crate::query::trace::TriggerMatch)
/// sub-line is inserted as the **first** pseudo-child whenever
/// [`TriggerMatch::sub_line_text`](crate::query::trace::TriggerMatch::sub_line_text)
/// returns `Some`. Mirrors the existing `╰─ if: <expr>` rendering convention
/// (sub-line below a node) but appears at the top of the children list rather
/// than the bottom.
fn render_entry_tree(
    entry: &TraceEntry,
    is_last_chain: &mut Vec<bool>,
    style: &TreeStyle,
    ui: &Ui,
    out: &mut String,
    marks: Option<&FindingMarks>,
) {
    let depth = is_last_chain.len();
    let actual = &entry.root;

    // Connector + label — same shape as render_node_tree's leading section.
    if depth == 0 {
        let connector = if style.unicode { "╭─ " } else { "+- " };
        out.push_str(&ui.muted(connector));
    } else {
        push_guide_prefix(out, &is_last_chain[..depth - 1], style, ui);
        let is_last = is_last_chain[depth - 1];
        let connector = match (is_last, style.unicode) {
            (true, true) => "╰─→ ",
            (false, true) => "├─→ ",
            (true, false) => "\\-> ",
            (false, false) => "|-> ",
        };
        out.push_str(&ui.muted(connector));
    }
    out.push_str(&label_for(actual, ui));
    out.push('\n');

    // Pseudo-children order: trigger sub-line, then finding `!` markers, then
    // real children. All share one position counter so `is_last` (and thus the
    // box-drawing connectors) stay correct.
    let trigger_text = entry.trigger.sub_line_text();
    let marker_lines = marks.map(|m| m.lines_for(actual)).unwrap_or(&[]);
    let children = node_children(actual);
    let trigger_count = if trigger_text.is_some() { 1 } else { 0 };
    let total = children.len() + trigger_count + marker_lines.len();
    let mut pos = 0;

    if let Some(text) = &trigger_text {
        is_last_chain.push(pos == total - 1);
        render_sub_line(text, is_last_chain, style, ui, out);
        is_last_chain.pop();
        pos += 1;
    }

    for line in marker_lines {
        is_last_chain.push(pos == total - 1);
        render_marker_line(line, is_last_chain, style, ui, out);
        is_last_chain.pop();
        pos += 1;
    }

    for child in children {
        is_last_chain.push(pos == total - 1);
        render_node_tree(child, is_last_chain, style, ui, out, marks);
        is_last_chain.pop();
        pos += 1;
    }
}

/// Render a metadata sub-line under the current node. Mirrors
/// [`render_if_lines`] for the connector glyph but takes a single-line
/// pre-formatted string (e.g. `types: labeled, opened`).
fn render_sub_line(
    text: &str,
    is_last_chain: &[bool],
    style: &TreeStyle,
    ui: &Ui,
    out: &mut String,
) {
    let depth = is_last_chain.len();
    debug_assert!(depth >= 1, "sub-line must have at least depth 1");

    push_guide_prefix(out, &is_last_chain[..depth - 1], style, ui);
    let is_last = is_last_chain[depth - 1];
    // No arrow — this is metadata, not a node. Mirrors `╰─ if: ...`.
    let connector = match (is_last, style.unicode) {
        (true, true) => "╰─ ",
        (false, true) => "├─ ",
        (true, false) => "\\- ",
        (false, false) => "|- ",
    };
    out.push_str(&ui.muted(connector));
    out.push_str(&ui.muted(text));
    out.push('\n');
}

/// Render an external-finding marker sub-line (`! [zizmor/high] rule (Lx)`)
/// under the current node. Same connector mechanics as [`render_sub_line`];
/// the `! ` lead is emphasized so findings stand out from muted metadata.
fn render_marker_line(
    text: &str,
    is_last_chain: &[bool],
    style: &TreeStyle,
    ui: &Ui,
    out: &mut String,
) {
    let depth = is_last_chain.len();
    debug_assert!(depth >= 1, "marker line must have at least depth 1");

    push_guide_prefix(out, &is_last_chain[..depth - 1], style, ui);
    let is_last = is_last_chain[depth - 1];
    let connector = match (is_last, style.unicode) {
        (true, true) => "╰─ ",
        (false, true) => "├─ ",
        (true, false) => "\\- ",
        (false, false) => "|- ",
    };
    out.push_str(&ui.muted(connector));
    out.push_str(&ui.strong("! "));
    out.push_str(text);
    out.push('\n');
}

fn render_node_tree(
    node: &TraceNode,
    is_last_chain: &mut Vec<bool>,
    style: &TreeStyle,
    ui: &Ui,
    out: &mut String,
    marks: Option<&FindingMarks>,
) {
    let depth = is_last_chain.len();

    // `Guarded` is rendered as its inner node + a synthetic `╰─ if: <expr>`
    // child line at depth+1. The wrapper itself contributes nothing visible.
    // `maybe_guarded` (in src/query/trace.rs) wraps at most once per edge, so
    // a single match is enough — no recursive unwrap needed.
    let (actual, if_expr) = match node {
        TraceNode::Guarded { inner, if_expr } => (inner.as_ref(), Some(if_expr.as_str())),
        other => (other, None),
    };

    // Connector + label for the actual node.
    if depth == 0 {
        let connector = if style.unicode { "╭─ " } else { "+- " };
        out.push_str(&ui.muted(connector));
    } else {
        push_guide_prefix(out, &is_last_chain[..depth - 1], style, ui);
        let is_last = is_last_chain[depth - 1];
        let connector = match (is_last, style.unicode) {
            (true, true) => "╰─→ ",
            (false, true) => "├─→ ",
            (true, false) => "\\-> ",
            (false, false) => "|-> ",
        };
        out.push_str(&ui.muted(connector));
    }
    out.push_str(&label_for(actual, ui));
    out.push('\n');

    // Pseudo-children: finding `!` markers, then real children, then the
    // optional synthetic if-line — all sharing one position counter so the
    // box-drawing connectors stay correct.
    let marker_lines = marks.map(|m| m.lines_for(actual)).unwrap_or(&[]);
    let children = node_children(actual);
    let total_count = marker_lines.len() + children.len() + if_expr.map(|_| 1).unwrap_or(0);
    let mut pos = 0;

    for line in marker_lines {
        is_last_chain.push(pos == total_count - 1);
        render_marker_line(line, is_last_chain, style, ui, out);
        is_last_chain.pop();
        pos += 1;
    }

    for child in children {
        is_last_chain.push(pos == total_count - 1);
        render_node_tree(child, is_last_chain, style, ui, out, marks);
        is_last_chain.pop();
        pos += 1;
    }

    if let Some(expr) = if_expr {
        is_last_chain.push(true);
        render_if_lines(expr, is_last_chain, style, ui, out);
        is_last_chain.pop();
    }
}

/// Render the synthetic `if:` child line(s). Multi-line `if:` (block scalar
/// `if: |`) is split per source line — the first line uses the `╰─` connector
/// (no arrow; this is metadata, not a real node), and continuation lines
/// align under the `if:` text via 7 spaces of indent (matching the visible
/// width of `╰─ if: `).
fn render_if_lines(
    expr: &str,
    is_last_chain: &[bool],
    style: &TreeStyle,
    ui: &Ui,
    out: &mut String,
) {
    let depth = is_last_chain.len();
    debug_assert!(depth >= 1, "if-line must have at least depth 1");

    // The connector glyph `╰─ ` is followed by `if: ` to make the 7-char
    // visible-width prefix that continuation lines must align under.
    let connector = if style.unicode { "╰─ " } else { "\\- " };
    let if_lead = "if: ";
    let cont_indent = " ".repeat(connector.chars().count() + if_lead.chars().count());

    let mut lines = expr.lines();
    let first = lines.next().unwrap_or("");

    // First (connector) line: <guide prefix><╰─ ><if: ><line0>
    push_guide_prefix(out, &is_last_chain[..depth - 1], style, ui);
    out.push_str(&ui.muted(connector));
    out.push_str(&ui.muted(&format!("{if_lead}{first}")));
    out.push('\n');

    // Continuation lines: <guide prefix><7 spaces><lineN>
    for line in lines {
        push_guide_prefix(out, &is_last_chain[..depth - 1], style, ui);
        out.push_str(&ui.muted(&cont_indent));
        out.push_str(&ui.muted(line));
        out.push('\n');
    }
}

/// Emit the muted vertical-guide prefix for a tree row at the given chain.
/// Each entry adds 4 visible columns: `│   ` when the level is non-last
/// (subsequent siblings continue below) or `    ` when the level is the last
/// child (no descender).
fn push_guide_prefix(out: &mut String, chain: &[bool], style: &TreeStyle, ui: &Ui) {
    let guide_continuation = if style.unicode { "│   " } else { "|   " };
    let guide_blank = "    ";
    for &was_last in chain {
        let g = if was_last {
            guide_blank
        } else {
            guide_continuation
        };
        out.push_str(&ui.muted(g));
    }
}

fn node_children(node: &TraceNode) -> &[TraceNode] {
    match node {
        TraceNode::Workflow { children, .. }
        | TraceNode::Action { children, .. }
        | TraceNode::Annotated { children, .. } => children,
        TraceNode::External(_)
        | TraceNode::ExternalWorkflow { .. }
        | TraceNode::Docker(_)
        | TraceNode::Cycle(_) => &[],
        TraceNode::Guarded { inner, .. } => node_children(inner),
    }
}

/// Render the label for a node.
///
/// The 3-segment shape is: `<bold name>  [<dim kind>]  <dim meta>`. Dangling
/// annotations and cycles colour the name red instead of bold so the hazard
/// stays visible at a glance even when the meta column is wide.
///
/// Callers pass the unwrapped (non-Guarded) node — `render_node_tree`
/// destructures `Guarded` upstream so the if-condition is rendered as a
/// synthetic child line (see `render_if_lines`), not as a label suffix.
fn label_for(node: &TraceNode, ui: &Ui) -> String {
    let (name, kind, meta) = label_segments(node);
    let danger = matches!(
        node,
        TraceNode::Annotated { dangling: true, .. } | TraceNode::Cycle(_)
    );
    let mut out = String::new();
    out.push_str(&ui.kind_styled_name(&name, kind, danger));
    if !kind.as_str().is_empty() {
        out.push_str("  ");
        out.push_str(&ui.tag_bracket(kind));
    }
    if !meta.is_empty() {
        out.push_str("  ");
        out.push_str(&ui.muted(&meta));
    }
    out
}

/// Decompose a `TraceNode` into the (name, kind, meta) triple used by
/// `label_for`. Extracted to keep the render path concise; single caller today.
fn label_segments(node: &TraceNode) -> (String, KindTag<'static>, String) {
    match node {
        TraceNode::Workflow { id, .. } => (id.0.clone(), KindTag::Workflow, String::new()),
        TraceNode::Action { id, .. } => (id.0.clone(), KindTag::Action, String::new()),
        TraceNode::External(e) => {
            let sub = e
                .subpath
                .as_deref()
                .map(|s| format!("/{s}"))
                .unwrap_or_default();
            let body = format!("{}/{}{}", e.owner, e.repo, sub);
            (body, KindTag::ExternalAction, format!("@{}", e.gitref))
        }
        TraceNode::ExternalWorkflow {
            owner,
            repo,
            path,
            gitref,
        } => (
            format!("{owner}/{repo}/{path}"),
            KindTag::ExternalWorkflow,
            format!("@{gitref}"),
        ),
        TraceNode::Docker(d) => {
            let host_prefix = d
                .host
                .as_deref()
                .map(|h| format!("{h}/"))
                .unwrap_or_default();
            let body = format!("{host_prefix}{}", d.image);
            let tail = d
                .tag
                .as_deref()
                .map(|t| format!(":{t}"))
                .unwrap_or_default();
            (body, KindTag::Docker, tail)
        }
        TraceNode::Annotated {
            verb,
            dangling,
            label,
            ..
        } => {
            let v = verb_str(*verb);
            let meta = if *dangling {
                format!("via {v} · dangling")
            } else {
                format!("via {v}")
            };
            (label.clone(), KindTag::Annotation, meta)
        }
        TraceNode::Cycle(target) => {
            let label = match target {
                CycleTarget::Workflow(id) => id.0.clone(),
                CycleTarget::Action(id) => id.0.clone(),
            };
            (label, KindTag::Cycle, "guard".into())
        }
        TraceNode::Guarded { .. } => {
            // Handled in `label_for`; recursion here would lose the if-suffix
            // attachment site.
            unreachable!("Guarded must be unwrapped in label_for");
        }
    }
}

fn external_str(e: &ExternalActionRef) -> String {
    let sub = e
        .subpath
        .as_deref()
        .map(|s| format!("/{s}"))
        .unwrap_or_default();
    format!("{}/{}{}@{}", e.owner, e.repo, sub, e.gitref)
}

fn verb_str(v: AnnotationVerb) -> &'static str {
    match v {
        AnnotationVerb::Dispatches => "dispatches",
        AnnotationVerb::Triggers => "triggers",
    }
}

/// Render a list of trace trees as a flat 5-column audit table.
/// Output never contains ANSI escape sequences regardless of TTY. The `unicode`
/// flag is accepted for CLI compatibility; the modern table view is deliberately
/// plain so it remains easy to grep and paste into CI logs. KIND values are
/// lowercase hyphenated identifiers (`wf` / `ac` / `ext-ac` / `ext-wf` /
/// `docker` / `ann` / `cyc`) matching the bracket tags used in the tree view.
pub fn render_table(entries: &[TraceEntry], _unicode: bool) -> String {
    let rows = table_rows(entries, None);
    render_rows(&["dep", "kind", "edge", "target", "note"], &rows)
}

/// `render_table` with the finding count folded into the `note` column.
pub fn render_table_with_findings(
    entries: &[TraceEntry],
    _unicode: bool,
    marks: &FindingMarks,
) -> String {
    let rows = table_rows(entries, Some(marks));
    render_rows(&["dep", "kind", "edge", "target", "note"], &rows)
}

pub fn render_markdown_table(entries: &[TraceEntry]) -> String {
    render_markdown_table_inner(entries, None)
}

/// `render_markdown_table` with the finding count folded into the Note column.
pub fn render_markdown_table_with_findings(entries: &[TraceEntry], marks: &FindingMarks) -> String {
    render_markdown_table_inner(entries, Some(marks))
}

fn render_markdown_table_inner(entries: &[TraceEntry], marks: Option<&FindingMarks>) -> String {
    let rows = table_rows(entries, marks);
    let headers = ["Dep", "Kind", "Edge", "Target", "Note"];
    let mut out = String::new();
    out.push_str("| Dep | Kind | Edge | Target | Note |\n");
    out.push_str("|---:|---|---|---|---|\n");
    for row in rows {
        debug_assert_eq!(row.len(), headers.len());
        out.push('|');
        for cell in row {
            out.push(' ');
            out.push_str(&markdown::table_cell(&cell));
            out.push_str(" |");
        }
        out.push('\n');
    }
    out
}

fn table_rows(entries: &[TraceEntry], marks: Option<&FindingMarks>) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for entry in entries {
        let entry_row_idx = rows.len();
        walk_table(&entry.root, 0, &mut rows);
        // Patch the root row's note column to embed the trigger sub-line text
        // when the matched trigger has activity types worth showing. Events
        // without activity types (push/schedule/...) leave the note unchanged.
        if let Some(text) = entry.trigger.sub_line_text() {
            if let Some(row) = rows.get_mut(entry_row_idx) {
                if row.len() == 5 {
                    row[4] = format!("entry, {text}");
                }
            }
        }
    }
    // Fold the per-node finding count into the note column (kept within the
    // existing 5-column contract — never adds a column). Matched by the row's
    // kind tag + target id.
    if let Some(marks) = marks {
        let wf_tag = KindTag::Workflow.as_str();
        let ac_tag = KindTag::Action.as_str();
        for row in &mut rows {
            if row.len() != 5 {
                continue;
            }
            let note = if row[1] == wf_tag {
                marks.workflows.get(&row[3]).map(|m| m.note.as_str())
            } else if row[1] == ac_tag {
                marks.actions.get(&row[3]).map(|m| m.note.as_str())
            } else {
                None
            };
            if let Some(note) = note {
                if !note.is_empty() {
                    row[4] = format!("{} [{note}]", row[4]);
                }
            }
        }
    }
    rows
}

fn walk_table(node: &TraceNode, depth: usize, rows: &mut Vec<Vec<String>>) {
    match node {
        TraceNode::Workflow { id, children } => {
            let note = if depth == 0 { "entry" } else { "reusable" };
            let edge = if depth == 0 { "entry" } else { "uses" };
            rows.push(vec![
                depth.to_string(),
                table_kind(KindTag::Workflow),
                edge.into(),
                id.0.clone(),
                note.into(),
            ]);
            for c in children {
                walk_table(c, depth + 1, rows);
            }
        }
        TraceNode::Action { id, children } => {
            rows.push(vec![
                depth.to_string(),
                table_kind(KindTag::Action),
                "uses".into(),
                id.0.clone(),
                "composite".into(),
            ]);
            for c in children {
                walk_table(c, depth + 1, rows);
            }
        }
        TraceNode::External(e) => {
            rows.push(vec![
                depth.to_string(),
                table_kind(KindTag::ExternalAction),
                "uses".into(),
                external_str(e),
                "-".into(),
            ]);
        }
        TraceNode::ExternalWorkflow {
            owner,
            repo,
            path,
            gitref,
        } => {
            rows.push(vec![
                depth.to_string(),
                table_kind(KindTag::ExternalWorkflow),
                "uses".into(),
                format!("{owner}/{repo}/{path}@{gitref}"),
                "-".into(),
            ]);
        }
        TraceNode::Docker(d) => {
            rows.push(vec![
                depth.to_string(),
                table_kind(KindTag::Docker),
                "uses".into(),
                d.display_str(),
                "-".into(),
            ]);
        }
        TraceNode::Annotated {
            verb,
            dangling,
            label,
            children,
        } => {
            let note = if *dangling { "dangling" } else { "resolved" };
            rows.push(vec![
                depth.to_string(),
                table_kind(KindTag::Annotation),
                verb_str(*verb).into(),
                label.clone(),
                note.into(),
            ]);
            for c in children {
                walk_table(c, depth + 1, rows);
            }
        }
        TraceNode::Cycle(target) => {
            let label = match target {
                CycleTarget::Workflow(id) => id.0.clone(),
                CycleTarget::Action(id) => id.0.clone(),
            };
            rows.push(vec![
                depth.to_string(),
                table_kind(KindTag::Cycle),
                "revisit".into(),
                label,
                "guard".into(),
            ]);
        }
        TraceNode::Guarded { if_expr, inner } => {
            // Render the inner node at the same depth, noting the guard
            // condition in the NOTE column. Match exhaustively so adding a
            // future TraceNode variant fails to compile here.
            let note = format!("if: {if_expr}");
            match inner.as_ref() {
                TraceNode::Workflow { id, children } => {
                    rows.push(vec![
                        depth.to_string(),
                        table_kind(KindTag::Workflow),
                        "uses".into(),
                        id.0.clone(),
                        note,
                    ]);
                    for c in children {
                        walk_table(c, depth + 1, rows);
                    }
                }
                TraceNode::Action { id, children } => {
                    rows.push(vec![
                        depth.to_string(),
                        table_kind(KindTag::Action),
                        "uses".into(),
                        id.0.clone(),
                        note,
                    ]);
                    for c in children {
                        walk_table(c, depth + 1, rows);
                    }
                }
                TraceNode::External(e) => {
                    rows.push(vec![
                        depth.to_string(),
                        table_kind(KindTag::ExternalAction),
                        "uses".into(),
                        external_str(e),
                        note,
                    ]);
                }
                TraceNode::ExternalWorkflow {
                    owner,
                    repo,
                    path,
                    gitref,
                } => {
                    rows.push(vec![
                        depth.to_string(),
                        table_kind(KindTag::ExternalWorkflow),
                        "uses".into(),
                        format!("{owner}/{repo}/{path}@{gitref}"),
                        note,
                    ]);
                }
                TraceNode::Docker(d) => {
                    rows.push(vec![
                        depth.to_string(),
                        table_kind(KindTag::Docker),
                        "uses".into(),
                        d.display_str(),
                        note,
                    ]);
                }
                TraceNode::Annotated {
                    verb,
                    label,
                    children,
                    ..
                } => {
                    rows.push(vec![
                        depth.to_string(),
                        table_kind(KindTag::Annotation),
                        verb_str(*verb).into(),
                        label.clone(),
                        note,
                    ]);
                    for c in children {
                        walk_table(c, depth + 1, rows);
                    }
                }
                TraceNode::Cycle(target) => {
                    let label = match target {
                        CycleTarget::Workflow(id) => id.0.clone(),
                        CycleTarget::Action(id) => id.0.clone(),
                    };
                    rows.push(vec![
                        depth.to_string(),
                        table_kind(KindTag::Cycle),
                        "revisit".into(),
                        label,
                        note,
                    ]);
                }
                // `maybe_guarded` only wraps once per edge, so nested Guarded
                // is not produced by the walker today. Recurse defensively
                // rather than panic if a future caller violates that.
                TraceNode::Guarded { .. } => walk_table(inner, depth, rows),
            }
        }
    }
}

fn table_kind(kind: KindTag<'_>) -> String {
    kind.as_str().into()
}

fn render_rows(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut widths: Vec<usize> = headers.iter().map(|h| display_width(h)).collect();
    for row in rows {
        debug_assert_eq!(
            row.len(),
            headers.len(),
            "table row width must match header width"
        );
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(display_width(cell));
        }
    }

    let mut out = String::new();
    for (i, header) in headers.iter().enumerate() {
        if i > 0 {
            out.push_str("  ");
        }
        if i == headers.len() - 1 {
            out.push_str(header);
        } else {
            out.push_str(&pad_display(header, widths[i]));
        }
    }
    out.push('\n');
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            if i == row.len() - 1 {
                out.push_str(cell);
            } else {
                out.push_str(&pad_display(cell, widths[i]));
            }
        }
        out.push('\n');
    }
    out
}

fn pad_display(text: &str, width: usize) -> String {
    let padding = width.saturating_sub(display_width(text));
    format!("{text}{}", " ".repeat(padding))
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ActionId, DockerRef, EventKind, ExternalActionRef, WorkflowId};
    use crate::query::trace::{TriggerMatch, TriggerTypesDisplay};
    use crate::ui::Ui;

    fn has_ansi(text: &str) -> bool {
        text.contains("\u{1b}[")
    }

    /// Wrap a [`TraceNode`] in a [`TraceEntry`] for tests. Defaults to
    /// `EventKind::Push` so `sub_line_text()` returns `None` and existing
    /// snapshot expectations stay unchanged.
    fn entry_for_test(node: TraceNode, event: EventKind) -> TraceEntry {
        TraceEntry {
            root: node,
            trigger: TriggerMatch {
                event,
                types: TriggerTypesDisplay::ImplicitAll,
            },
        }
    }

    /// Convenience to convert a `Vec<TraceNode>` to `Vec<TraceEntry>` with all
    /// entries defaulting to `EventKind::Push`.
    fn entries_from(roots: Vec<TraceNode>) -> Vec<TraceEntry> {
        roots
            .into_iter()
            .map(|n| entry_for_test(n, EventKind::Push))
            .collect()
    }

    fn ext(owner: &str, repo: &str, gitref: &str) -> TraceNode {
        TraceNode::External(ExternalActionRef {
            owner: owner.into(),
            repo: repo.into(),
            subpath: None,
            gitref: gitref.into(),
        })
    }

    fn small_tree() -> Vec<TraceNode> {
        vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![
                TraceNode::Annotated {
                    verb: AnnotationVerb::Dispatches,
                    dangling: false,
                    label: ".github/workflows/build.yml".into(),
                    children: vec![TraceNode::Workflow {
                        id: WorkflowId(".github/workflows/build.yml".into()),
                        children: vec![ext("actions", "checkout", "v4")],
                    }],
                },
                TraceNode::Annotated {
                    verb: AnnotationVerb::Triggers,
                    dangling: true,
                    label: "missing.yml".into(),
                    children: vec![],
                },
                TraceNode::Cycle(CycleTarget::Workflow(WorkflowId(
                    ".github/workflows/ci.yml".into(),
                ))),
                TraceNode::Action {
                    id: ActionId(".github/actions/setup".into()),
                    children: vec![],
                },
                ext("actions", "cache", "v3"),
                TraceNode::ExternalWorkflow {
                    owner: "acme".into(),
                    repo: "shared".into(),
                    path: ".github/workflows/deploy.yml".into(),
                    gitref: "v1".into(),
                },
            ],
        }]
    }

    #[test]
    fn render_tree_unicode_uses_rounded_branch_chars() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(out.contains("╭─ "), "missing root open: {out}");
        assert!(out.contains("├─→ "), "missing mid branch: {out}");
        assert!(out.contains("╰─→ "), "missing terminal branch: {out}");
        assert!(out.contains("│"), "missing continuation guide: {out}");
    }

    #[test]
    fn render_tree_ascii_uses_pipe_and_arrow_chars() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: false },
            &Ui::plain_for_test(),
        );
        assert!(out.contains("+- "), "missing ASCII root open: {out}");
        assert!(out.contains("|-> "), "missing ASCII mid branch: {out}");
        assert!(
            out.contains("\\-> "),
            "missing ASCII terminal branch: {out}"
        );
        assert!(!out.contains('├'), "ASCII must not contain Unicode: {out}");
        assert!(!out.contains('╭'), "ASCII must not contain Unicode: {out}");
    }

    #[test]
    fn render_tree_root_uses_open_connector_not_inline_marker() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        let first_line = out.lines().next().expect("at least one line");
        assert!(
            first_line.starts_with("╭─ "),
            "root should start with `╭─ `, got: {first_line:?}"
        );
        assert!(
            !out.contains("== "),
            "old `==` root marker must not appear: {out}"
        );
        assert!(
            !out.contains("◆"),
            "old `◆` rich root marker must not appear: {out}"
        );
    }

    #[test]
    fn render_tree_uses_three_segment_label_format() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        // Plain-mode root line should contain bold name + bracketed kind tag.
        assert!(
            out.contains(".github/workflows/ci.yml  [wf]"),
            "expected `<name>  [wf]` 3-segment label for root: {out}"
        );
    }

    #[test]
    fn render_tree_external_workflow_uses_ext_wf_tag() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains("acme/shared/.github/workflows/deploy.yml  [ext-wf]  @v1"),
            "expected ext-wf 3-segment label: {out}"
        );
    }

    #[test]
    fn render_tree_external_action_uses_ext_ac_tag() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains("actions/cache  [ext-ac]  @v3"),
            "expected ext-ac 3-segment label: {out}"
        );
    }

    #[test]
    fn render_tree_local_action_uses_ac_tag() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains(".github/actions/setup  [ac]"),
            "expected ac 3-segment label: {out}"
        );
    }

    #[test]
    fn render_tree_annotation_uses_ann_tag_and_via_meta() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains(".github/workflows/build.yml  [ann]  via dispatches"),
            "expected ann label with via meta: {out}"
        );
    }

    #[test]
    fn render_tree_dangling_annotation_meta_includes_dangling() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains("missing.yml  [ann]  via triggers · dangling"),
            "expected dangling meta segment: {out}"
        );
    }

    #[test]
    fn render_tree_cycle_uses_cyc_tag_and_guard_meta() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains(".github/workflows/ci.yml  [cyc]  guard"),
            "expected cyc 3-segment label: {out}"
        );
    }

    #[test]
    fn render_tree_dangling_annotation_name_is_red_in_color_mode() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::color_for_test(),
        );
        assert!(has_ansi(&out), "expected ANSI: {out}");
        // The name token `missing.yml` should be wrapped in danger (red+bold).
        assert!(
            out.contains("\u{1b}[1m\u{1b}[31mmissing.yml\u{1b}[0m"),
            "expected red bold name for dangling: {out}"
        );
    }

    #[test]
    fn render_tree_color_disabled_no_ansi() {
        let out = render_tree(
            &entries_from(small_tree()),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(!has_ansi(&out), "must not contain ESC byte: {out}");
    }

    #[test]
    fn render_tree_inserts_blank_line_between_multiple_roots() {
        let mut roots = small_tree();
        roots.push(TraceNode::Workflow {
            id: WorkflowId(".github/workflows/release.yml".into()),
            children: vec![ext("actions", "checkout", "v4")],
        });
        let out = render_tree(
            &entries_from(roots),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains("\n\n╭─ .github/workflows/release.yml"),
            "second root should be preceded by blank line + `╭─ `: {out}"
        );
    }

    #[test]
    fn render_tree_no_per_node_spacer() {
        // Per-node parent → child spacer was dropped in R4 to tighten internal
        // density. A single root → single child must now emit just two lines.
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![ext("actions", "checkout", "v4")],
        }];
        let out = render_tree(
            &entries_from(tree),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines[0].starts_with("╭─ "),
            "line 0 should be root: {:?}",
            lines.first()
        );
        assert!(
            lines[1].starts_with("╰─→ "),
            "line 1 should be the only child immediately after the root: {:?}",
            lines.get(1)
        );
        assert!(
            !out.contains("\n│\n"),
            "must not contain the old per-node `│` spacer: {out}"
        );
    }

    #[test]
    fn render_tree_top_level_workflows_get_column_zero_spacer() {
        // Top-level workflow siblings under an event root keep a 1-line `│`
        // spacer between them so each entry workflow has breathing room.
        let tree = vec![
            TraceNode::Workflow {
                id: WorkflowId(".github/workflows/a.yml".into()),
                children: vec![],
            },
            TraceNode::Workflow {
                id: WorkflowId(".github/workflows/b.yml".into()),
                children: vec![],
            },
        ];
        let summary = vec!["filters=none".to_string()];
        let event_meta = EventMeta {
            event: "push",
            summary: &summary,
        };
        let out = render_tree(
            &entries_from(tree),
            Some(event_meta),
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains("├─→ .github/workflows/a.yml"),
            "first sibling uses mid connector: {out}"
        );
        assert!(
            out.contains("\n│\n╰─→ .github/workflows/b.yml"),
            "second top-level workflow must be preceded by a `│` spacer line: {out}"
        );
    }

    #[test]
    fn render_tree_event_meta_emits_event_as_synthetic_root_with_summary() {
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![ext("actions", "checkout", "v4")],
        }];
        let summary = vec!["filters=none".to_string()];
        let event_meta = EventMeta {
            event: "push",
            summary: &summary,
        };
        let out = render_tree(
            &entries_from(tree),
            Some(event_meta),
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(
            lines[0],
            "╭─ push   (filters=none)",
            "line 0 should be the synthetic event root with parens summary: {:?}",
            lines.first()
        );
        assert!(
            lines[1].starts_with("╰─→ "),
            "line 1 should be the workflow directly below the event (no spacer for single root): {:?}",
            lines.get(1)
        );
    }

    #[test]
    fn render_tree_event_meta_with_empty_summary_drops_parens() {
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![],
        }];
        let event_meta = EventMeta {
            event: "schedule",
            summary: &[],
        };
        let out = render_tree(
            &entries_from(tree),
            Some(event_meta),
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        let first = out.lines().next().expect("at least one line");
        assert_eq!(
            first, "╭─ schedule",
            "empty summary must omit the parens entirely: {first:?}"
        );
    }

    #[test]
    fn render_tree_event_meta_groups_multiple_roots_under_event() {
        let tree = vec![
            TraceNode::Workflow {
                id: WorkflowId(".github/workflows/a.yml".into()),
                children: vec![],
            },
            TraceNode::Workflow {
                id: WorkflowId(".github/workflows/b.yml".into()),
                children: vec![],
            },
            TraceNode::Workflow {
                id: WorkflowId(".github/workflows/c.yml".into()),
                children: vec![],
            },
        ];
        let summary = vec!["filters=none".to_string()];
        let event_meta = EventMeta {
            event: "push",
            summary: &summary,
        };
        let out = render_tree(
            &entries_from(tree),
            Some(event_meta),
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        // Roots become children of the event: first/middle use ├─→, last uses ╰─→.
        assert!(
            out.contains("├─→ .github/workflows/a.yml"),
            "first sibling should use mid connector: {out}"
        );
        assert!(
            out.contains("├─→ .github/workflows/b.yml"),
            "second sibling should use mid connector: {out}"
        );
        assert!(
            out.contains("╰─→ .github/workflows/c.yml"),
            "last sibling should use terminal connector: {out}"
        );
        // Summary lives at the synthetic root only.
        assert!(out.starts_with("╭─ push"), "event row leads: {out}");
        assert!(
            out.contains("(filters=none)"),
            "summary parens at event row: {out}"
        );
    }

    #[test]
    fn render_tree_event_meta_ascii_falls_back_to_plus_root() {
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![],
        }];
        let summary = vec!["filters=none".to_string()];
        let event_meta = EventMeta {
            event: "push",
            summary: &summary,
        };
        let out = render_tree(
            &entries_from(tree),
            Some(event_meta),
            &TreeStyle { unicode: false },
            &Ui::plain_for_test(),
        );
        assert!(
            out.starts_with("+- push"),
            "ASCII mode must use `+- ` connector for event root: {out}"
        );
        assert!(
            !out.contains('╭'),
            "ASCII mode must not contain Unicode: {out}"
        );
    }

    #[test]
    fn render_table_renders_columns_and_lowercase_kinds() {
        let out = render_table(&entries_from(small_tree()), true);
        for header in ["dep", "kind", "edge", "target", "note"] {
            assert!(out.contains(header), "header {header} missing: {out}");
        }
        for kind in ["wf", "ac", "ext-ac", "ext-wf", "ann", "cyc"] {
            assert!(out.contains(kind), "KIND {kind} missing: {out}");
        }
        for old in ["WF", "AC", "EX", "EW", "DO", "ANN", "CYC"] {
            assert!(
                !out.split_whitespace().any(|tok| tok == old),
                "old uppercase KIND `{old}` must not appear: {out}"
            );
        }
        assert!(out.contains("dangling"), "dangling note missing: {out}");
        assert!(out.contains("guard"), "cycle guard note missing: {out}");
    }

    #[test]
    fn render_rows_aligns_columns_by_unicode_display_width() {
        let out = render_rows(
            &["target", "note"],
            &[
                vec!["界".into(), "wide".into()],
                vec!["ascii".into(), "plain".into()],
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "target  note");
        assert_eq!(lines[1], "界      wide");
        assert_eq!(lines[2], "ascii   plain");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "table row width must match header width")]
    fn render_rows_debug_asserts_row_width_contract() {
        let _ = render_rows(&["one"], &[vec!["one".into(), "two".into()]]);
    }

    #[test]
    fn render_table_ascii_preset_remains_plain() {
        let out = render_table(&entries_from(small_tree()), false);
        assert!(out.starts_with("dep  kind"), "plain header missing: {out}");
        assert!(
            !out.contains("┌─"),
            "plain table must not contain Unicode border: {out}"
        );
    }

    fn docker_leaf(image: &str, tag: Option<&str>) -> TraceNode {
        TraceNode::Docker(DockerRef {
            host: None,
            image: image.into(),
            tag: tag.map(|t| t.into()),
        })
    }

    #[test]
    fn render_tree_docker_leaf_uses_docker_tag_and_colon_meta() {
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![docker_leaf("alpine", Some("3.8"))],
        }];
        let out = render_tree(
            &entries_from(tree),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        assert!(
            out.contains("alpine  [docker]  :3.8"),
            "expected docker label: {out}"
        );
    }

    #[test]
    fn render_table_docker_leaf_kind_lowercase() {
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![docker_leaf("alpine", Some("3.8"))],
        }];
        let out = render_table(&entries_from(tree), true);
        assert!(out.contains("docker"), "KIND docker missing: {out}");
        assert!(out.contains("alpine:3.8"), "Docker target missing: {out}");
    }

    #[test]
    fn render_table_emits_if_note_for_guarded_external_workflow() {
        // Regression: walk_table previously only recognized Guarded inner =
        // Workflow / Action / External. EW / Docker fell through the catch-all
        // recursion arm, dropping the if_expr from the NOTE column.
        let if_expr = "github.event_name == 'push'";
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![
                TraceNode::Guarded {
                    if_expr: if_expr.into(),
                    inner: Box::new(TraceNode::ExternalWorkflow {
                        owner: "acme".into(),
                        repo: "shared".into(),
                        path: ".github/workflows/deploy.yml".into(),
                        gitref: "v1".into(),
                    }),
                },
                TraceNode::Guarded {
                    if_expr: if_expr.into(),
                    inner: Box::new(docker_leaf("alpine", Some("3.8"))),
                },
            ],
        }];
        let out = render_table(&entries_from(tree), true);
        let expected_note = format!("if: {if_expr}");
        assert!(out.contains("ext-wf"), "KIND ext-wf missing: {out}");
        assert!(out.contains("docker"), "KIND docker missing: {out}");
        assert_eq!(
            out.matches(&expected_note).count(),
            2,
            "expected two `{expected_note}` notes (ext-wf + docker), got: {out}"
        );
    }

    #[test]
    fn render_table_emits_if_note_for_guarded_annotated_and_cycle() {
        let if_expr = "github.event_name == 'push'";
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![
                TraceNode::Guarded {
                    if_expr: if_expr.into(),
                    inner: Box::new(TraceNode::Annotated {
                        verb: AnnotationVerb::Dispatches,
                        dangling: false,
                        label: ".github/workflows/build.yml".into(),
                        children: vec![],
                    }),
                },
                TraceNode::Guarded {
                    if_expr: if_expr.into(),
                    inner: Box::new(TraceNode::Cycle(CycleTarget::Workflow(WorkflowId(
                        ".github/workflows/ci.yml".into(),
                    )))),
                },
            ],
        }];
        let out = render_table(&entries_from(tree), true);
        let expected_note = format!("if: {if_expr}");
        assert!(out.contains("ann"), "KIND ann missing: {out}");
        assert!(out.contains("cyc"), "KIND cyc missing: {out}");
        assert_eq!(
            out.matches(&expected_note).count(),
            2,
            "expected two `{expected_note}` notes (ann + cyc), got: {out}"
        );
    }

    #[test]
    fn render_table_root_edge_is_entry() {
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![],
        }];
        let out = render_table(&entries_from(tree), true);
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines.len() >= 2,
            "expected header + at least one row: {out}"
        );
        let cols: Vec<&str> = lines[1].split_whitespace().collect();
        assert_eq!(cols[0], "0", "expected DEP=0 for root row: {out}");
        assert_eq!(cols[1], "wf", "expected KIND=wf for root row: {out}");
        assert_eq!(cols[2], "entry", "expected EDGE=entry for root row: {out}");
    }

    #[test]
    fn render_tree_guarded_emits_if_as_synthetic_child_line() {
        // Single-line if: lives on its own row under the guarded node, with
        // `╰─` connector (no arrow). Parent connector is unaffected by the
        // synthetic child.
        let if_expr = "github.event_name == 'push'";
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![TraceNode::Guarded {
                if_expr: if_expr.into(),
                inner: Box::new(TraceNode::ExternalWorkflow {
                    owner: "acme".into(),
                    repo: "shared".into(),
                    path: ".github/workflows/deploy.yml".into(),
                    gitref: "v1".into(),
                }),
            }],
        }];
        let out = render_tree(
            &entries_from(tree),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        // The Guarded wrapper renders as the inner ExternalWorkflow at depth=1,
        // and the if-expr appears as a `╰─ if: <expr>` synthetic child at
        // depth=2. Concrete prefix locks the guide-column alignment.
        assert!(
            out.contains("    ╰─ if: github.event_name == 'push'"),
            "expected `╰─ if: <expr>` child line under the guarded node: {out}"
        );
        // U+00B7 (middle dot) followed by `if:` was the old suffix form.
        let old_suffix = "\u{00B7} if:";
        assert!(
            !out.contains(old_suffix),
            "old middle-dot if-suffix form must not appear: {out}"
        );
    }

    #[test]
    fn render_tree_guarded_emits_multiline_if_with_seven_char_alignment() {
        // Multi-line if: each source line gets its own row. Continuation lines
        // align under the `if:` text column (7 chars past the connector,
        // matching the visible width of `╰─ if: `).
        let if_expr =
            "github.event_name == 'push'\n&& startsWith(github.ref, 'refs/tags/')\n&& always()";
        let tree = vec![TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![TraceNode::Guarded {
                if_expr: if_expr.into(),
                inner: Box::new(TraceNode::ExternalWorkflow {
                    owner: "acme".into(),
                    repo: "shared".into(),
                    path: ".github/workflows/deploy.yml".into(),
                    gitref: "v1".into(),
                }),
            }],
        }];
        let out = render_tree(
            &entries_from(tree),
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        );
        // Concrete prefix assertion that locks 7-char `if:` column alignment +
        // guide continuation. The guarded node is the only/last child of the
        // root, so its own guide column is blank (4 spaces). The connector
        // line uses `╰─ if: ` (7 visible chars), and the two continuation
        // lines use 7 spaces in its place — total 4+7 = 11 chars before the
        // continuation text, vs 4+3 = 7 chars before `if:` on the connector
        // line.
        let expected = "    ╰─ if: github.event_name == 'push'\n           && startsWith(github.ref, 'refs/tags/')\n           && always()";
        assert!(
            out.contains(expected),
            "expected concrete multi-line if-prefix alignment.\nexpected substring:\n{expected}\nactual:\n{out}"
        );
    }
}
