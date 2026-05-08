use crate::ir::*;
use crate::query::walk::{for_each_outgoing_edge, Edge, Node};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OrphanResult {
    /// Reusable workflows (those with `on.workflow_call`) that no entry-point
    /// workflow transitively reaches.
    pub unused_workflows: Vec<WorkflowId>,
    /// Local action manifests (composite / JavaScript / Docker) that no caller
    /// references. The kind is carried alongside the id so downstream consumers
    /// (CLI text label, JSON output) can distinguish action kinds without
    /// re-reading the IR. Mirrors `ImpactResult.actions` shape.
    pub unused_actions: Vec<(ActionId, ActionKind)>,
    /// `(callee_id, input_name)` for declared inputs whose callee body never
    /// references them. Only callees with a `workflow_call` trigger (workflows)
    /// or `runs.using: composite` (actions) are scanned. Bails out
    /// conservatively when the callee uses dynamic input access
    /// (`${{ inputs[var] }}`).
    pub unreferenced_inputs: Vec<(String, String)>,
    /// `(callee_id, output_name)` for declared outputs that no caller
    /// references via `needs.<job>.outputs.<X>` (workflow callees) or
    /// `steps.<id>.outputs.<X>` (composite callees). Only callees that have
    /// at least one local callsite are scanned.
    pub unused_outputs: Vec<(String, String)>,
}

pub fn orphans(ir: &Ir) -> OrphanResult {
    let wf_lookup: HashMap<&str, &Workflow> =
        ir.workflows.iter().map(|w| (w.id.0.as_str(), w)).collect();
    let act_lookup: HashMap<&str, &LocalAction> =
        ir.actions.iter().map(|c| (c.id.0.as_str(), c)).collect();

    // Reachability BFS from every entry-point workflow.
    let mut reachable_wf: HashSet<String> = HashSet::new();
    let mut reachable_action: HashSet<String> = HashSet::new();

    let mut queue: VecDeque<NodeRef> = VecDeque::new();
    for wf in &ir.workflows {
        if wf.triggers.iter().any(|t| t.is_entry_point()) {
            queue.push_back(NodeRef::Workflow(wf.id.0.clone()));
            reachable_wf.insert(wf.id.0.clone());
        }
    }

    while let Some(node) = queue.pop_front() {
        let walk_node = match &node {
            NodeRef::Workflow(id) => wf_lookup.get(id.as_str()).copied().map(Node::Workflow),
            NodeRef::Action(id) => act_lookup.get(id.as_str()).copied().map(Node::Action),
        };
        let Some(walk_node) = walk_node else {
            continue;
        };
        for_each_outgoing_edge(walk_node, |ctx| match ctx.edge {
            Edge::Annotation(ann) => {
                if let AnnotationResolution::Resolved { target } = &ann.resolution {
                    if reachable_wf.insert(target.0.clone()) {
                        queue.push_back(NodeRef::Workflow(target.0.clone()));
                    }
                }
            }
            Edge::CallsWorkflow(call) => {
                if let WorkflowRef::Local(target) = &call.workflow_ref {
                    if reachable_wf.insert(target.0.clone()) {
                        queue.push_back(NodeRef::Workflow(target.0.clone()));
                    }
                }
            }
            Edge::Uses(uses) => match uses {
                UsesRef::LocalWorkflow(WorkflowId(p)) => {
                    if reachable_wf.insert(p.clone()) {
                        queue.push_back(NodeRef::Workflow(p.clone()));
                    }
                }
                UsesRef::LocalAction(ActionId(p)) => {
                    if reachable_action.insert(p.clone()) {
                        queue.push_back(NodeRef::Action(p.clone()));
                    }
                }
                UsesRef::External { .. } | UsesRef::Docker(_) => {}
            },
        });
    }

    let mut unused_workflows: Vec<WorkflowId> = ir
        .workflows
        .iter()
        .filter(|wf| {
            // Only "reusable" workflows can be orphans (entry-points are always
            // implicitly reachable via their trigger).
            wf.triggers
                .iter()
                .any(|t| t.event == EventKind::WorkflowCall)
                && !wf.triggers.iter().any(|t| t.is_entry_point())
                && !reachable_wf.contains(&wf.id.0)
        })
        .map(|wf| wf.id.clone())
        .collect();
    unused_workflows.sort_by(|a, b| a.0.cmp(&b.0));

    let mut unused_actions: Vec<(ActionId, ActionKind)> = ir
        .actions
        .iter()
        .filter(|c| !reachable_action.contains(&c.id.0))
        .map(|c| (c.id.clone(), c.kind.clone()))
        .collect();
    unused_actions.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));

    // Declared-input / declared-output unused-detection scan phase.
    //
    // Independent of the reachability BFS above (Plan: "BFS の出力に依存しない
    // 別 scan phase を追加"). The trigger-based filters embedded in
    // `Workflow::inputs()` / `Workflow::outputs()` (only workflows that declare
    // `on.workflow_call` expose signature) and the composite-only `kind` check
    // gate the scan with these constraints:
    //
    // - `workflow_dispatch`-only workflows are excluded (their `inputs()`
    //   returns `None`).
    // - JS / Docker actions skip the input-reference scan
    //   (`!matches!(c.kind, ActionKind::Composite)`).
    // - Dynamic input access (`${{ inputs[var] }}`) inside a callee body bails
    //   out conservatively for `unreferenced_inputs` (`scan_input_refs`).
    // - `step.env` values participate in the expression scan, along with
    //   workflow- and job-level `env:`, `job.if_expr`, `wf.run_name`,
    //   workflow / job `concurrency`, `job.environment` (name / url),
    //   `job.container` and `job.services` (image / options / env). This
    //   matches the issue #115 / #109 expansion: any string-valued field that
    //   may carry a `${{ inputs.X }}` expression contributes to the scan,
    //   without execution-time evaluation.
    let workflow_exprs: BTreeMap<&str, Vec<String>> = ir
        .workflows
        .iter()
        .map(|w| (w.id.0.as_str(), collect_workflow_expressions(w)))
        .collect();
    let composite_exprs: BTreeMap<&str, Vec<String>> = ir
        .actions
        .iter()
        .map(|c| (c.id.0.as_str(), collect_composite_expressions(c)))
        .collect();

    let mut unreferenced_inputs: Vec<(String, String)> = Vec::new();
    for callee in &ir.workflows {
        let Some(declared) = callee.inputs() else {
            continue;
        };
        let exprs = workflow_exprs
            .get(callee.id.0.as_str())
            .cloned()
            .unwrap_or_default();
        unreferenced_inputs.extend(scan_input_refs(declared, &exprs, &callee.id.0));
    }
    for callee in &ir.actions {
        if !matches!(callee.kind, ActionKind::Composite) {
            continue;
        }
        let exprs = composite_exprs
            .get(callee.id.0.as_str())
            .cloned()
            .unwrap_or_default();
        unreferenced_inputs.extend(scan_input_refs(&callee.inputs, &exprs, &callee.id.0));
    }
    unreferenced_inputs.sort();

    let mut unused_outputs: Vec<(String, String)> = Vec::new();
    for callee in &ir.workflows {
        let Some(outputs) = callee.outputs() else {
            continue;
        };
        if outputs.is_empty() {
            continue;
        }
        let callsites = collect_workflow_callsites(ir, &callee.id.0);
        if callsites.is_empty() {
            continue;
        }
        unused_outputs.extend(detect_unused_workflow_outputs(
            callee,
            outputs,
            &callsites,
            &workflow_exprs,
        ));
    }
    for callee in &ir.actions {
        if !matches!(callee.kind, ActionKind::Composite) {
            continue;
        }
        if callee.outputs.is_empty() {
            continue;
        }
        let callsites = collect_composite_callsites(ir, &callee.id.0);
        if callsites.is_empty() {
            continue;
        }
        unused_outputs.extend(detect_unused_composite_outputs(
            callee,
            &callsites,
            &workflow_exprs,
            &composite_exprs,
        ));
    }
    unused_outputs.sort();

    OrphanResult {
        unused_workflows,
        unused_actions,
        unreferenced_inputs,
        unused_outputs,
    }
}

#[derive(Debug)]
enum NodeRef {
    Workflow(String),
    Action(String),
}

// ---------------------------------------------------------------------------
// Helpers for declared-but-unused signature element detection.
//
// These detect declared signature elements (inputs / outputs) that are not
// consumed across the workflow estate. They run independently of the
// reachability BFS above; iteration is over all workflows / composites in the
// IR, gated by trigger-kind filters: only workflows with `on.workflow_call`
// expose `inputs()` / `outputs()`, and only `runs.using: composite` actions
// participate in the input-reference scan (JS / Docker actions are skipped
// because their inputs are consumed outside YAML).
// ---------------------------------------------------------------------------

pub(crate) fn collect_workflow_expressions(wf: &Workflow) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(outputs) = wf.outputs() {
        for o in outputs {
            if let Some(v) = o.value.as_ref() {
                out.push(v.clone());
            }
        }
    }
    if let Some(s) = wf.run_name.as_ref() {
        out.push(s.clone());
    }
    for v in wf.env.values() {
        out.push(v.clone());
    }
    if let Some(c) = wf.concurrency.as_ref() {
        push_concurrency(c, &mut out);
    }
    for job in &wf.jobs {
        if let Some(s) = job.if_expr.as_ref() {
            out.push(s.clone());
        }
        for v in job.outputs.values() {
            out.push(v.clone());
        }
        for v in job.env.values() {
            out.push(v.clone());
        }
        if let Some(c) = job.concurrency.as_ref() {
            push_concurrency(c, &mut out);
        }
        if let Some(env) = job.environment.as_ref() {
            out.push(env.name.clone());
            if let Some(url) = env.url.as_ref() {
                out.push(url.clone());
            }
        }
        if let Some(container) = job.container.as_ref() {
            push_container(container, &mut out);
        }
        for service in job.services.values() {
            push_container(service, &mut out);
        }
        if let Some(call) = &job.calls_workflow {
            for v in call.with.values() {
                out.push(v.clone());
            }
        }
        for step in &job.steps {
            collect_step_expressions(step, &mut out);
        }
    }
    out
}

pub(crate) fn collect_composite_expressions(comp: &LocalAction) -> Vec<String> {
    let mut out = Vec::new();
    for o in &comp.outputs {
        if let Some(v) = o.value.as_ref() {
            out.push(v.clone());
        }
    }
    for step in &comp.steps {
        collect_step_expressions(step, &mut out);
    }
    out
}

fn collect_step_expressions(step: &Step, out: &mut Vec<String>) {
    if let Some(s) = step.run.as_ref() {
        out.push(s.clone());
    }
    if let Some(s) = step.if_expr.as_ref() {
        out.push(s.clone());
    }
    for v in step.with.values() {
        out.push(v.clone());
    }
    for v in step.env.values() {
        out.push(v.clone());
    }
    if let Some(s) = step.continue_on_error.as_ref() {
        out.push(s.clone());
    }
}

fn push_concurrency(c: &Concurrency, out: &mut Vec<String>) {
    out.push(c.group.clone());
    // `cancel_in_progress` is `Option<bool>` on the IR (the parser collapses
    // both literal booleans and expression strings into a bool when possible);
    // there is no raw expression carrier here, so nothing to scan.
}

fn push_container(c: &JobContainer, out: &mut Vec<String>) {
    out.push(c.image.clone());
    if let Some(opts) = c.options.as_ref() {
        out.push(opts.clone());
    }
    for v in c.env.values() {
        out.push(v.clone());
    }
    if let Some(creds) = c.credentials.as_ref() {
        out.push(creds.username.clone());
        out.push(creds.password.clone());
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ExpressionRefs {
    pub(crate) direct_inputs: BTreeSet<String>,
    pub(crate) needs_outputs: BTreeSet<(String, String)>,
    pub(crate) steps_outputs: BTreeSet<(String, String)>,
    pub(crate) dynamic_inputs: bool,
    pub(crate) dynamic_needs: bool,
    pub(crate) dynamic_steps: bool,
}

pub(crate) fn scan_expressions(strings: &[String]) -> ExpressionRefs {
    let mut refs = ExpressionRefs::default();
    for s in strings {
        scan_one(s, &mut refs);
    }
    refs
}

fn scan_one(text: &str, refs: &mut ExpressionRefs) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if &bytes[i..i + 3] == b"${{" {
            let start = i + 3;
            let mut j = start;
            while j + 1 < bytes.len() && &bytes[j..j + 2] != b"}}" {
                j += 1;
            }
            if j + 1 >= bytes.len() {
                break;
            }
            scan_block(&text[start..j], refs);
            i = j + 2;
        } else {
            i += 1;
        }
    }
}

fn scan_block(block: &str, refs: &mut ExpressionRefs) {
    for name in find_idents_after(block, "inputs.") {
        refs.direct_inputs.insert(name.to_string());
    }
    if block.contains("inputs[") {
        refs.dynamic_inputs = true;
    }
    for (job, name) in find_chain_pairs(block, "needs.") {
        refs.needs_outputs.insert((job, name));
    }
    if block.contains("needs[") {
        refs.dynamic_needs = true;
    }
    for (id, name) in find_chain_pairs(block, "steps.") {
        refs.steps_outputs.insert((id, name));
    }
    if block.contains("steps[") {
        refs.dynamic_steps = true;
    }
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn find_idents_after<'a>(text: &'a str, prefix: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut start = 0;
    while let Some(rel) = text[start..].find(prefix) {
        let begin = start + rel + prefix.len();
        let end = text[begin..]
            .bytes()
            .position(|b| !is_ident_byte(b))
            .map(|p| begin + p)
            .unwrap_or(text.len());
        if end > begin {
            out.push(&text[begin..end]);
        }
        start = end.max(begin + 1);
    }
    out
}

fn find_chain_pairs(text: &str, root_dot: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let outputs_marker = ".outputs.";
    let mut start = 0;
    while let Some(rel) = text[start..].find(root_dot) {
        let begin = start + rel + root_dot.len();
        let id_end = text[begin..]
            .bytes()
            .position(|b| !is_ident_byte(b))
            .map(|p| begin + p)
            .unwrap_or(text.len());
        if id_end == begin {
            start = begin + 1;
            continue;
        }
        let id = text[begin..id_end].to_string();
        let after = &text[id_end..];
        if let Some(rest) = after.strip_prefix(outputs_marker) {
            let name_begin = id_end + outputs_marker.len();
            let name_end = rest
                .bytes()
                .position(|b| !is_ident_byte(b))
                .map(|p| name_begin + p)
                .unwrap_or(text.len());
            if name_end > name_begin {
                out.push((id, text[name_begin..name_end].to_string()));
                start = name_end;
                continue;
            }
        }
        start = id_end.max(begin + 1);
    }
    out
}

/// Detect declared inputs that are not referenced inside the callee body.
/// Returns `Vec<(callee_id, input_name)>`. Bails out conservatively when the
/// callee uses dynamic input access (`${{ inputs[var] }}`).
pub(crate) fn scan_input_refs(
    declared: &[InputDecl],
    exprs: &[String],
    callee_id: &str,
) -> Vec<(String, String)> {
    let refs = scan_expressions(exprs);
    if refs.dynamic_inputs {
        return Vec::new();
    }
    let mut out = Vec::new();
    for d in declared {
        if !refs.direct_inputs.contains(&d.name) {
            out.push((callee_id.to_string(), d.name.clone()));
        }
    }
    out
}

pub(crate) struct WorkflowCallsite {
    pub(crate) caller: String,
    pub(crate) caller_job: String,
}

pub(crate) struct CompositeCallsite {
    pub(crate) caller: String,
    pub(crate) caller_is_workflow: bool,
    pub(crate) step_id: Option<String>,
}

pub(crate) fn collect_workflow_callsites(ir: &Ir, callee_id: &str) -> Vec<WorkflowCallsite> {
    let mut out = Vec::new();
    for wf in &ir.workflows {
        for job in &wf.jobs {
            if let Some(call) = &job.calls_workflow {
                if let WorkflowRef::Local(target) = &call.workflow_ref {
                    if target.0 == callee_id {
                        out.push(WorkflowCallsite {
                            caller: wf.id.0.clone(),
                            caller_job: job.id.0.clone(),
                        });
                    }
                }
            }
        }
    }
    out
}

pub(crate) fn collect_composite_callsites(ir: &Ir, callee_id: &str) -> Vec<CompositeCallsite> {
    let mut out = Vec::new();
    for wf in &ir.workflows {
        for job in &wf.jobs {
            for step in &job.steps {
                if let Some(UsesRef::LocalAction(action_id)) = &step.uses {
                    if action_id.0 == callee_id {
                        out.push(CompositeCallsite {
                            caller: wf.id.0.clone(),
                            caller_is_workflow: true,
                            step_id: step.id.as_ref().map(|s| s.0.clone()),
                        });
                    }
                }
            }
        }
    }
    for comp in &ir.actions {
        for step in &comp.steps {
            if let Some(UsesRef::LocalAction(action_id)) = &step.uses {
                if action_id.0 == callee_id {
                    out.push(CompositeCallsite {
                        caller: comp.id.0.clone(),
                        caller_is_workflow: false,
                        step_id: step.id.as_ref().map(|s| s.0.clone()),
                    });
                }
            }
        }
    }
    out
}

/// Detect declared workflow outputs that no caller references via
/// `needs.<job>.outputs.<X>`. Returns `Vec<(callee_id, output_name)>`.
/// Dynamic `needs[...]` access on the caller is treated as covering all
/// outputs (conservative: do not report).
pub(crate) fn detect_unused_workflow_outputs(
    callee: &Workflow,
    outputs: &[OutputDecl],
    callsites: &[WorkflowCallsite],
    workflow_exprs: &BTreeMap<&str, Vec<String>>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for output in outputs {
        let mut used = false;
        for cs in callsites {
            let Some(exprs) = workflow_exprs.get(cs.caller.as_str()) else {
                continue;
            };
            let refs = scan_expressions(exprs);
            if refs.dynamic_needs {
                used = true;
                break;
            }
            if refs
                .needs_outputs
                .contains(&(cs.caller_job.clone(), output.name.clone()))
            {
                used = true;
                break;
            }
        }
        if !used {
            out.push((callee.id.0.clone(), output.name.clone()));
        }
    }
    out
}

/// Detect declared composite outputs that no caller references via
/// `steps.<id>.outputs.<X>`. Returns `Vec<(callee_id, output_name)>`.
/// Dynamic `steps[...]` access on the caller is treated as covering all
/// outputs (conservative: do not report).
pub(crate) fn detect_unused_composite_outputs(
    callee: &LocalAction,
    callsites: &[CompositeCallsite],
    workflow_exprs: &BTreeMap<&str, Vec<String>>,
    composite_exprs: &BTreeMap<&str, Vec<String>>,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for output in &callee.outputs {
        let mut used = false;
        for cs in callsites {
            let exprs = if cs.caller_is_workflow {
                workflow_exprs.get(cs.caller.as_str())
            } else {
                composite_exprs.get(cs.caller.as_str())
            };
            let Some(exprs) = exprs else {
                continue;
            };
            let refs = scan_expressions(exprs);
            if refs.dynamic_steps {
                used = true;
                break;
            }
            if let Some(id) = &cs.step_id {
                if refs
                    .steps_outputs
                    .contains(&(id.clone(), output.name.clone()))
                {
                    used = true;
                    break;
                }
            }
        }
        if !used {
            out.push((callee.id.0.clone(), output.name.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refs_of(s: &str) -> ExpressionRefs {
        scan_expressions(&[s.to_string()])
    }

    #[test]
    fn scanner_extracts_inputs_needs_steps() {
        let refs = refs_of(
            "echo ${{ inputs.foo }} ${{ needs.build.outputs.url }} ${{ steps.s1.outputs.bar }}",
        );
        assert!(refs.direct_inputs.contains("foo"));
        assert!(refs
            .needs_outputs
            .contains(&("build".to_string(), "url".to_string())));
        assert!(refs
            .steps_outputs
            .contains(&("s1".to_string(), "bar".to_string())));
        assert!(!refs.dynamic_inputs);
        assert!(!refs.dynamic_needs);
        assert!(!refs.dynamic_steps);
    }

    #[test]
    fn scanner_handles_multiple_refs_in_one_block() {
        let refs = refs_of("${{ format('{0}-{1}', inputs.a, inputs.b) }}");
        assert!(refs.direct_inputs.contains("a"));
        assert!(refs.direct_inputs.contains("b"));
        assert!(!refs.dynamic_inputs);
    }

    #[test]
    fn scanner_ignores_text_outside_expression_blocks() {
        let refs = refs_of("inputs.outside should be ignored, only ${{ inputs.real }} counts");
        assert_eq!(refs.direct_inputs.len(), 1);
        assert!(refs.direct_inputs.contains("real"));
    }

    #[test]
    fn scanner_flags_dynamic_inputs_access() {
        let refs = refs_of("${{ inputs[github.event.inputs.name] }}");
        assert!(refs.dynamic_inputs);
    }

    #[test]
    fn scanner_flags_dynamic_needs_and_steps() {
        let refs = refs_of("${{ needs[matrix.job].outputs.x }} ${{ steps[fmt].outputs.y }}");
        assert!(refs.dynamic_needs);
        assert!(refs.dynamic_steps);
    }

    #[test]
    fn scanner_handles_unterminated_expression() {
        let refs = refs_of("${{ inputs.foo");
        assert!(refs.direct_inputs.is_empty());
    }

    // ----- orphans() integration cases ----------------------------------------
    //
    // Inline IR-builder helpers (`mk_workflow`, `mk_job`, `mk_step`, `mk_action`)
    // construct IR fragments via direct struct literals so each test pins ONE
    // branch of `orphans()` (reachability BFS, kind filter, multi-caller
    // reachability, declared-but-unused-input scan).

    use std::path::PathBuf;

    fn empty_pos() -> SourcePos {
        SourcePos {
            file: PathBuf::new(),
            line: None,
        }
    }

    fn mk_step_uses_action(action_id: &str) -> Step {
        Step {
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
            source: empty_pos(),
            annotations: Vec::new(),
        }
    }

    fn mk_step_uses_workflow(workflow_id: &str) -> Step {
        Step {
            index: 0,
            id: None,
            name: None,
            uses: Some(UsesRef::LocalWorkflow(WorkflowId(workflow_id.into()))),
            run: None,
            if_expr: None,
            with: Default::default(),
            env: Default::default(),
            shell: None,
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
            source: empty_pos(),
            annotations: Vec::new(),
        }
    }

    fn mk_step_uses_action_with_id(action_id: &str, step_id: Option<&str>) -> Step {
        Step {
            index: 0,
            id: step_id.map(|id| StepId(id.into())),
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
            source: empty_pos(),
            annotations: Vec::new(),
        }
    }

    fn mk_run_step(run: &str) -> Step {
        Step {
            index: 1,
            id: None,
            name: None,
            uses: None,
            run: Some(run.into()),
            if_expr: None,
            with: Default::default(),
            env: Default::default(),
            shell: None,
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
            source: empty_pos(),
            annotations: Vec::new(),
        }
    }

    fn mk_job(wf_id: &str, id: &str, steps: Vec<Step>, calls: Option<CallsWorkflow>) -> Job {
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
            annotations: Vec::new(),
        }
    }

    fn workflow_call_trigger(inputs: Vec<InputDecl>, outputs: Vec<OutputDecl>) -> TriggerSpec {
        TriggerSpec {
            event: EventKind::WorkflowCall,
            branches: RefFilter::None,
            tags: RefFilter::None,
            paths: RefFilter::None,
            types: None,
            extras: Some(EventExtras::WorkflowCall {
                inputs,
                outputs,
                secrets: vec![],
            }),
        }
    }

    fn input(name: &str) -> InputDecl {
        InputDecl {
            name: name.into(),
            required: false,
            default: None,
            input_type: None,
        }
    }

    fn output(name: &str) -> OutputDecl {
        OutputDecl {
            name: name.into(),
            value: Some(format!("${{{{ steps.build.outputs.{name} }}}}")),
        }
    }

    fn mk_workflow(id: &str, triggers: Vec<TriggerSpec>, jobs: Vec<Job>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: empty_pos(),
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

    fn mk_action(
        id: &str,
        kind: ActionKind,
        inputs: Vec<InputDecl>,
        steps: Vec<Step>,
    ) -> LocalAction {
        LocalAction {
            id: ActionId(id.into()),
            source: empty_pos(),
            name: None,
            kind,
            inputs,
            outputs: vec![],
            steps,
            annotations: Vec::new(),
        }
    }

    fn ir(workflows: Vec<Workflow>, actions: Vec<LocalAction>) -> Ir {
        Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows,
            actions,
            external_actions: vec![],
        }
    }

    #[test]
    fn orphans_marks_unreachable_reusable_workflow() {
        // build.yml is workflow_call-only and no entry-point reaches it.
        let ci = mk_workflow(
            ".github/workflows/ci.yml",
            vec![TriggerSpec::bare(EventKind::Push)],
            vec![],
        );
        let build = mk_workflow(
            ".github/workflows/build.yml",
            vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            vec![],
        );
        let result = orphans(&ir(vec![ci, build], vec![]));
        assert_eq!(
            result.unused_workflows,
            vec![WorkflowId(".github/workflows/build.yml".into())],
        );
    }

    #[test]
    fn orphans_annotation_edge_marks_reusable_workflow_reachable() {
        let mut ci = mk_workflow(
            ".github/workflows/ci.yml",
            vec![TriggerSpec::bare(EventKind::Push)],
            vec![],
        );
        ci.annotations.push(Annotation {
            verb: AnnotationVerb::Dispatches,
            resolution: AnnotationResolution::Resolved {
                target: WorkflowId(".github/workflows/build.yml".into()),
            },
            source_line: 1,
        });
        let build = mk_workflow(
            ".github/workflows/build.yml",
            vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            vec![],
        );

        let result = orphans(&ir(vec![ci, build], vec![]));

        assert!(
            result.unused_workflows.is_empty(),
            "annotation-reached workflow must not be orphan: {:?}",
            result.unused_workflows
        );
    }

    #[test]
    fn orphans_local_workflow_step_marks_reusable_workflow_reachable() {
        let ci = mk_workflow(
            ".github/workflows/ci.yml",
            vec![TriggerSpec::bare(EventKind::Push)],
            vec![mk_job(
                ".github/workflows/ci.yml",
                "call-build",
                vec![mk_step_uses_workflow(".github/workflows/build.yml")],
                None,
            )],
        );
        let build = mk_workflow(
            ".github/workflows/build.yml",
            vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            vec![],
        );

        let result = orphans(&ir(vec![ci, build], vec![]));

        assert!(
            result.unused_workflows.is_empty(),
            "local workflow step must mark callee reachable: {:?}",
            result.unused_workflows
        );
    }

    #[test]
    fn orphans_excludes_reachable_reusable_workflow() {
        // ci.yml (entry) calls build.yml (workflow_call) → build.yml is reached.
        let ci = mk_workflow(
            ".github/workflows/ci.yml",
            vec![TriggerSpec::bare(EventKind::Push)],
            vec![mk_job(
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
        let build = mk_workflow(
            ".github/workflows/build.yml",
            vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            vec![],
        );
        let result = orphans(&ir(vec![ci, build], vec![]));
        assert!(
            result.unused_workflows.is_empty(),
            "build.yml is reached, must not be orphan: {:?}",
            result.unused_workflows
        );
    }

    #[test]
    fn orphans_treats_dynamic_needs_access_as_workflow_output_usage() {
        let callee = mk_workflow(
            ".github/workflows/build.yml",
            vec![workflow_call_trigger(vec![], vec![output("artifact")])],
            vec![],
        );
        let caller = mk_workflow(
            ".github/workflows/ci.yml",
            vec![TriggerSpec::bare(EventKind::Push)],
            vec![
                mk_job(
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
                ),
                mk_job(
                    ".github/workflows/ci.yml",
                    "consume",
                    vec![mk_run_step(
                        "echo ${{ needs[matrix.job].outputs.artifact }}",
                    )],
                    None,
                ),
            ],
        );

        let result = orphans(&ir(vec![caller, callee], vec![]));

        assert!(
            result.unused_outputs.is_empty(),
            "dynamic needs access should conservatively cover workflow outputs: {:?}",
            result.unused_outputs
        );
    }

    #[test]
    fn orphans_treats_dynamic_steps_access_as_composite_output_usage() {
        let mut action = mk_action(
            ".github/actions/build",
            ActionKind::Composite,
            vec![],
            vec![],
        );
        action.outputs = vec![output("artifact")];
        let caller = mk_workflow(
            ".github/workflows/ci.yml",
            vec![TriggerSpec::bare(EventKind::Push)],
            vec![mk_job(
                ".github/workflows/ci.yml",
                "build",
                vec![
                    mk_step_uses_action_with_id(".github/actions/build", Some("build")),
                    mk_run_step("echo ${{ steps[matrix.step].outputs.artifact }}"),
                ],
                None,
            )],
        );

        let result = orphans(&ir(vec![caller], vec![action]));

        assert!(
            result.unused_outputs.is_empty(),
            "dynamic steps access should conservatively cover composite outputs: {:?}",
            result.unused_outputs
        );
    }

    #[test]
    fn orphans_step_without_id_cannot_satisfy_composite_output_usage() {
        let mut action = mk_action(
            ".github/actions/build",
            ActionKind::Composite,
            vec![],
            vec![],
        );
        action.outputs = vec![output("artifact")];
        let caller = mk_workflow(
            ".github/workflows/ci.yml",
            vec![TriggerSpec::bare(EventKind::Push)],
            vec![mk_job(
                ".github/workflows/ci.yml",
                "build",
                vec![
                    mk_step_uses_action_with_id(".github/actions/build", None),
                    mk_run_step("echo ${{ steps.build.outputs.artifact }}"),
                ],
                None,
            )],
        );

        let result = orphans(&ir(vec![caller], vec![action]));

        assert_eq!(
            result.unused_outputs,
            vec![(".github/actions/build".to_string(), "artifact".to_string(),)],
        );
    }

    #[test]
    fn collect_workflow_expressions_includes_non_step_input_carriers() {
        let mut with = BTreeMap::new();
        with.insert(
            "config".to_string(),
            "${{ inputs.from_call_with }}".to_string(),
        );
        let mut container_env = BTreeMap::new();
        container_env.insert(
            "TOKEN".to_string(),
            "${{ inputs.from_container_env }}".to_string(),
        );
        let container = JobContainer {
            image: "${{ inputs.from_container_image }}".to_string(),
            credentials: Some(JobContainerCredentials {
                username: "${{ inputs.from_container_user }}".to_string(),
                password: "${{ inputs.from_container_password }}".to_string(),
            }),
            env: container_env,
            ports: vec![],
            volumes: vec![],
            options: Some("${{ inputs.from_container_options }}".to_string()),
        };
        let mut services = BTreeMap::new();
        services.insert(
            "db".to_string(),
            JobContainer {
                image: "${{ inputs.from_service_image }}".to_string(),
                credentials: None,
                env: Default::default(),
                ports: vec![],
                volumes: vec![],
                options: None,
            },
        );
        let mut job = mk_job(
            ".github/workflows/reuse.yml",
            "build",
            vec![Step {
                continue_on_error: Some("${{ inputs.from_continue_on_error }}".to_string()),
                ..mk_run_step("echo ok")
            }],
            Some(CallsWorkflow {
                workflow_ref: WorkflowRef::Local(WorkflowId(".github/workflows/child.yml".into())),
                with,
                secrets: SecretsPass::None,
            }),
        );
        job.environment = Some(JobEnvironment {
            name: "prod".to_string(),
            url: Some("${{ inputs.from_environment_url }}".to_string()),
        });
        job.concurrency = Some(Concurrency {
            group: "${{ inputs.from_job_concurrency }}".to_string(),
            cancel_in_progress: None,
        });
        job.container = Some(container);
        job.services = services;
        let mut wf = mk_workflow(
            ".github/workflows/reuse.yml",
            vec![workflow_call_trigger(
                vec![
                    input("from_run_name"),
                    input("from_workflow_concurrency"),
                    input("from_job_concurrency"),
                    input("from_environment_url"),
                    input("from_container_image"),
                    input("from_container_user"),
                    input("from_container_password"),
                    input("from_container_env"),
                    input("from_container_options"),
                    input("from_service_image"),
                    input("from_call_with"),
                    input("from_continue_on_error"),
                    input("unused"),
                ],
                vec![],
            )],
            vec![job],
        );
        wf.run_name = Some("${{ inputs.from_run_name }}".to_string());
        wf.concurrency = Some(Concurrency {
            group: "${{ inputs.from_workflow_concurrency }}".to_string(),
            cancel_in_progress: None,
        });

        let exprs = collect_workflow_expressions(&wf);
        let unreferenced =
            scan_input_refs(wf.inputs().expect("workflow_call inputs"), &exprs, &wf.id.0);

        assert_eq!(
            unreferenced,
            vec![(
                ".github/workflows/reuse.yml".to_string(),
                "unused".to_string()
            )],
        );
    }

    #[test]
    fn orphans_lists_unused_actions_with_kind_label() {
        // setup composite + js Docker action: neither has a caller.
        let setup = mk_action(
            ".github/actions/setup",
            ActionKind::Composite,
            vec![],
            vec![],
        );
        let js = mk_action(
            ".github/actions/notify",
            ActionKind::JavaScript {
                node_version: "20".into(),
            },
            vec![],
            vec![],
        );
        let result = orphans(&ir(vec![], vec![setup, js]));
        assert_eq!(result.unused_actions.len(), 2);
        // Sorted by id.
        assert_eq!(result.unused_actions[0].0 .0, ".github/actions/notify");
        assert!(matches!(
            result.unused_actions[0].1,
            ActionKind::JavaScript { .. }
        ));
        assert_eq!(result.unused_actions[1].0 .0, ".github/actions/setup");
        assert_eq!(result.unused_actions[1].1, ActionKind::Composite);
    }

    #[test]
    fn orphans_action_used_by_three_workflows_is_not_orphan() {
        // Multi-caller scenario: 3 entry-point workflows all use the same
        // composite. Reachability must register the action across all callers.
        let setup = mk_action(
            ".github/actions/setup",
            ActionKind::Composite,
            vec![],
            vec![],
        );
        let mk_caller = |id: &str| -> Workflow {
            mk_workflow(
                id,
                vec![TriggerSpec::bare(EventKind::Push)],
                vec![mk_job(
                    id,
                    "run",
                    vec![mk_step_uses_action(".github/actions/setup")],
                    None,
                )],
            )
        };
        let result = orphans(&ir(
            vec![
                mk_caller(".github/workflows/a.yml"),
                mk_caller(".github/workflows/b.yml"),
                mk_caller(".github/workflows/c.yml"),
            ],
            vec![setup],
        ));
        assert!(
            result.unused_actions.is_empty(),
            "action with 3 callers must not be orphan: {:?}",
            result.unused_actions
        );
    }

    #[test]
    fn orphans_reports_unreferenced_input_in_composite() {
        // Composite declares input `unused_param` but no `${{ inputs.unused_param }}`
        // reference appears anywhere in its body.
        let action = mk_action(
            ".github/actions/setup",
            ActionKind::Composite,
            vec![InputDecl {
                name: "unused_param".into(),
                required: false,
                default: None,
                input_type: None,
            }],
            vec![],
        );
        let result = orphans(&ir(vec![], vec![action]));
        assert_eq!(
            result.unreferenced_inputs,
            vec![(
                ".github/actions/setup".to_string(),
                "unused_param".to_string(),
            )],
        );
    }

    #[test]
    fn orphans_skips_input_scan_for_javascript_actions() {
        // JS / Docker actions consume inputs outside YAML; we must not flag
        // their declared inputs even when no reference appears in the manifest.
        let js = mk_action(
            ".github/actions/probe",
            ActionKind::JavaScript {
                node_version: "20".into(),
            },
            vec![InputDecl {
                name: "endpoint".into(),
                required: false,
                default: None,
                input_type: None,
            }],
            vec![],
        );
        // Reference the action so it isn't flagged as unused.
        let wf = mk_workflow(
            ".github/workflows/ci.yml",
            vec![TriggerSpec::bare(EventKind::Push)],
            vec![mk_job(
                ".github/workflows/ci.yml",
                "run",
                vec![mk_step_uses_action(".github/actions/probe")],
                None,
            )],
        );
        let result = orphans(&ir(vec![wf], vec![js]));
        assert!(
            result.unreferenced_inputs.is_empty(),
            "JS action input must not be flagged as unreferenced: {:?}",
            result.unreferenced_inputs
        );
    }
}
