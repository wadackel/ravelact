use crate::ir::*;
use crate::query::walk::{for_each_outgoing_edge, Edge, Node, SourceTier};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Tree node returned by [`trace`]. Renders via the helpers in
/// [`crate::query::trace_render`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TraceNode {
    Workflow {
        id: WorkflowId,
        children: Vec<TraceNode>,
    },
    Action {
        id: ActionId,
        children: Vec<TraceNode>,
    },
    External(ExternalActionRef),
    /// Cross-repo reusable workflow call (`uses: owner/repo/.github/workflows/X.yml@ref`).
    /// Distinct from [`TraceNode::External`] which represents external *actions*
    /// (`uses: owner/repo@ref`). The IR preserves this distinction via
    /// [`WorkflowRef::External`] vs [`UsesRef::External`]; this variant surfaces
    /// it in the render layer.
    ExternalWorkflow {
        owner: String,
        repo: String,
        /// Full path within the repo, e.g. `.github/workflows/deploy.yml`.
        path: String,
        gitref: String,
    },
    /// Leaf node for a `docker://` action reference. Docker images are opaque
    /// (no further traversal); this node makes them visible in trace / dump output.
    Docker(DockerRef),
    /// Edge introduced by an `# ravelact:dispatches` / `# ravelact:triggers`
    /// comment. Resolved annotations recurse into the target workflow;
    /// dangling annotations are emitted as a leaf with `dangling = true` so
    /// the renderer can flag them.
    Annotated {
        verb: AnnotationVerb,
        dangling: bool,
        /// Workflow id when resolved; raw target text when dangling.
        label: String,
        children: Vec<TraceNode>,
    },
    /// Emitted in place of an empty leaf when the traversal hits a node it is
    /// already visiting. Distinguishes a real leaf (no further `uses:`) from a
    /// cycle-guard truncation in the rendered output.
    Cycle(CycleTarget),
    /// Wraps a child node whose parent job and/or step carries an `if:`
    /// expression. When both job-level and step-level guards apply, they are
    /// combined with logical AND (`(job_if) && (step_if)`) to mirror GitHub
    /// Actions' short-circuit semantics. The `if_expr` is kept as a raw string
    /// (no evaluation). The renderer appends `(if: <expr>)` to the inner
    /// node's label so reviewers can see which edges are conditionally reached.
    Guarded {
        if_expr: String,
        inner: Box<TraceNode>,
    },
}

/// Discriminator for [`TraceNode::Cycle`] — preserves the typed id of the
/// already-visited target so consumers can recover what kind of node was being
/// re-entered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CycleTarget {
    Workflow(WorkflowId),
    Action(ActionId),
}

/// Result of a single entry-point match returned by [`trace`]. Pairs the
/// rendered tree (`root`) with metadata about the trigger declaration that
/// matched.
///
/// `root` is always [`TraceNode::Workflow`] — `trace()` only ever pushes
/// `walk_workflow` results as roots, never external workflows or other
/// variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceEntry {
    pub root: TraceNode,
    pub trigger: TriggerMatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceJsonEntry {
    pub root: TraceJsonNode,
    pub trigger: TraceJsonTrigger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TraceJsonTrigger {
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub types: Option<TraceJsonTriggerTypes>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TraceJsonTriggerTypes {
    Explicit { values: Vec<String> },
    ImplicitAll,
    ImplicitDefault { values: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TraceJsonNode {
    Workflow {
        id: String,
        children: Vec<TraceJsonNode>,
    },
    Action {
        id: String,
        children: Vec<TraceJsonNode>,
    },
    ExternalAction {
        owner: String,
        repo: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        subpath: Option<String>,
        gitref: String,
    },
    ExternalWorkflow {
        owner: String,
        repo: String,
        path: String,
        gitref: String,
    },
    Docker {
        image: String,
    },
    Annotated {
        verb: AnnotationVerb,
        dangling: bool,
        label: String,
        children: Vec<TraceJsonNode>,
    },
    Cycle {
        target_kind: &'static str,
        target: String,
    },
    Guarded {
        if_expr: String,
        inner: Box<TraceJsonNode>,
    },
}

pub fn trace_json_entries(entries: &[TraceEntry]) -> Vec<TraceJsonEntry> {
    entries
        .iter()
        .map(|entry| TraceJsonEntry {
            root: trace_json_node(&entry.root),
            trigger: TraceJsonTrigger {
                event: entry.trigger.event.name().to_string(),
                types: entry.trigger.types.json_display(&entry.trigger.event),
            },
        })
        .collect()
}

/// Trigger declaration that allowed an entry workflow to match the trace
/// query. Surfaced so renderers can show which `types:` declaration (explicit,
/// implicit-all, implicit-default subset) selected the workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerMatch {
    pub event: EventKind,
    pub types: TriggerTypesDisplay,
}

/// Three-way classification of how `types:` was declared on the matched
/// trigger. Mirrors GitHub Actions semantics directly:
///
/// - [`Self::Explicit`]: user wrote `types: [a, b]` — list captured verbatim.
/// - [`Self::ImplicitAll`]: user omitted `types:` and the event has no
///   default subset (e.g. `issues:` fires on every activity type).
/// - [`Self::ImplicitDefault`]: user omitted `types:` and GitHub Actions
///   applies a default subset (currently only `pull_request` /
///   `pull_request_target` with `[opened, synchronize, reopened]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerTypesDisplay {
    Explicit(Vec<String>),
    ImplicitAll,
    ImplicitDefault(Vec<String>),
}

impl TriggerTypesDisplay {
    /// Derive the display classification from a [`TriggerSpec`]. Pure
    /// transformation — no rendering happens here.
    pub fn from_trigger(spec: &TriggerSpec) -> Self {
        match (&spec.types, spec.event.default_activity_subset()) {
            (Some(explicit), _) => TriggerTypesDisplay::Explicit(explicit.clone()),
            (None, Some(default)) => TriggerTypesDisplay::ImplicitDefault(
                default.iter().map(|&s| s.to_string()).collect(),
            ),
            (None, None) => TriggerTypesDisplay::ImplicitAll,
        }
    }

    fn json_display(&self, event: &EventKind) -> Option<TraceJsonTriggerTypes> {
        match self {
            TriggerTypesDisplay::Explicit(values) => Some(TraceJsonTriggerTypes::Explicit {
                values: values.clone(),
            }),
            TriggerTypesDisplay::ImplicitDefault(values) => {
                Some(TraceJsonTriggerTypes::ImplicitDefault {
                    values: values.clone(),
                })
            }
            TriggerTypesDisplay::ImplicitAll if event.supports_activity_types() => {
                Some(TraceJsonTriggerTypes::ImplicitAll)
            }
            TriggerTypesDisplay::ImplicitAll => None,
        }
    }
}

fn trace_json_node(node: &TraceNode) -> TraceJsonNode {
    match node {
        TraceNode::Workflow { id, children } => TraceJsonNode::Workflow {
            id: id.0.clone(),
            children: children.iter().map(trace_json_node).collect(),
        },
        TraceNode::Action { id, children } => TraceJsonNode::Action {
            id: id.0.clone(),
            children: children.iter().map(trace_json_node).collect(),
        },
        TraceNode::External(e) => TraceJsonNode::ExternalAction {
            owner: e.owner.clone(),
            repo: e.repo.clone(),
            subpath: e.subpath.clone(),
            gitref: e.gitref.clone(),
        },
        TraceNode::ExternalWorkflow {
            owner,
            repo,
            path,
            gitref,
        } => TraceJsonNode::ExternalWorkflow {
            owner: owner.clone(),
            repo: repo.clone(),
            path: path.clone(),
            gitref: gitref.clone(),
        },
        TraceNode::Docker(d) => TraceJsonNode::Docker {
            image: d.display_str(),
        },
        TraceNode::Annotated {
            verb,
            dangling,
            label,
            children,
        } => TraceJsonNode::Annotated {
            verb: *verb,
            dangling: *dangling,
            label: label.clone(),
            children: children.iter().map(trace_json_node).collect(),
        },
        TraceNode::Cycle(target) => {
            let (target_kind, target) = match target {
                CycleTarget::Workflow(id) => ("workflow", id.0.clone()),
                CycleTarget::Action(id) => ("action", id.0.clone()),
            };
            TraceJsonNode::Cycle {
                target_kind,
                target,
            }
        }
        TraceNode::Guarded { if_expr, inner } => TraceJsonNode::Guarded {
            if_expr: if_expr.clone(),
            inner: Box::new(trace_json_node(inner)),
        },
    }
}

impl TriggerMatch {
    /// Text the renderer should attach to an entry workflow as a sub-line
    /// (`├─ {text}` / `╰─ {text}` in tree mode, `entry, {text}` in table
    /// mode). Returns `None` for events that have no activity-type concept at
    /// all (e.g. `push`, `schedule`, `workflow_dispatch`) — there is nothing
    /// meaningful to display, so the caller should skip the sub-line entirely.
    pub fn sub_line_text(&self) -> Option<String> {
        match &self.types {
            TriggerTypesDisplay::Explicit(values) => Some(format!("types: {}", values.join(", "))),
            TriggerTypesDisplay::ImplicitDefault(values) => {
                Some(format!("types: {} (default)", values.join(", ")))
            }
            TriggerTypesDisplay::ImplicitAll => {
                if self.event.supports_activity_types() {
                    Some("types: any".to_string())
                } else {
                    None
                }
            }
        }
    }
}

/// Returns one [`TraceEntry`] per entry-point workflow whose `on:` matches
/// `event`.
///
/// All filter slices follow OR-within / AND-across semantics. An empty slice
/// disables that filter (event-name match alone is sufficient for that axis):
///
/// - `types` is OR-matched against the trigger's activity types via
///   [`TriggerSpec::matches_activity`].
/// - `branches`, `tags`, `paths` are OR-matched against the trigger's
///   `RefFilter` fields via [`RefFilter::matches`]. A trigger whose filter is
///   `RefFilter::None` accepts every value (matches GitHub Actions behavior:
///   no filter declared = trigger fires for every ref / path).
///
/// When multiple triggers on the same workflow could match (event name +
/// every filter satisfied), the **first IR-order trigger that passes all
/// filters** is selected for the [`TriggerMatch`]. This is deterministic and
/// keeps the reported trigger consistent with the filter result. Multiple
/// triggers under a single `on:` key are virtually impossible per YAML map
/// semantics, so this rarely matters in practice.
pub fn trace(
    ir: &Ir,
    event: &str,
    types: &[String],
    branches: &[String],
    tags: &[String],
    paths: &[String],
) -> Vec<TraceEntry> {
    let wf_lookup: HashMap<&str, &Workflow> =
        ir.workflows.iter().map(|w| (w.id.0.as_str(), w)).collect();
    let act_lookup: HashMap<&str, &LocalAction> =
        ir.actions.iter().map(|c| (c.id.0.as_str(), c)).collect();

    let mut entries = Vec::new();
    for wf in &ir.workflows {
        let matched = wf.triggers.iter().find(|t| {
            t.event_name() == event
                && t.is_entry_point()
                && (types.is_empty() || types.iter().any(|act| t.matches_activity(act)))
                && (branches.is_empty() || branches.iter().any(|b| t.branches.matches(b)))
                && (tags.is_empty() || tags.iter().any(|x| t.tags.matches(x)))
                && (paths.is_empty() || paths.iter().any(|p| t.paths.matches(p)))
        });
        if let Some(trigger_spec) = matched {
            let mut visiting_wf = HashSet::new();
            let mut visiting_action = HashSet::new();
            let root = walk_workflow(
                &wf_lookup,
                &act_lookup,
                &wf.id,
                &mut visiting_wf,
                &mut visiting_action,
            );
            entries.push(TraceEntry {
                root,
                trigger: TriggerMatch {
                    event: trigger_spec.event.clone(),
                    types: TriggerTypesDisplay::from_trigger(trigger_spec),
                },
            });
        }
    }
    entries
}

fn walk_workflow(
    wf_lookup: &HashMap<&str, &Workflow>,
    act_lookup: &HashMap<&str, &LocalAction>,
    id: &WorkflowId,
    visiting_wf: &mut HashSet<String>,
    visiting_action: &mut HashSet<String>,
) -> TraceNode {
    if !visiting_wf.insert(id.0.clone()) {
        // cycle guard
        return TraceNode::Cycle(CycleTarget::Workflow(id.clone()));
    }
    let mut children = Vec::new();
    if let Some(wf) = wf_lookup.get(id.0.as_str()) {
        // The walker yields edges in the historical trace order (workflow.annotations,
        // then per-job: job.annotations, calls_workflow, per-step annotations, uses).
        // Tier-specific guard wrapping is re-derived from `ctx.source`.
        let edges: Vec<(SourceTier<'_>, Edge<'_>)> = {
            let mut buf = Vec::new();
            for_each_outgoing_edge(Node::Workflow(wf), |ctx| buf.push((ctx.source, ctx.edge)));
            buf
        };
        for (source, edge) in edges {
            // Re-derive guard wrappers for this tier-edge pair. The original
            // `walk_workflow` only wrapped step-tier edges with the combined
            // `(job_if, step_if)` guard and `job.calls_workflow` with `job_if`
            // alone; annotations carried by workflow- or job-level tiers were
            // never wrapped. The match below preserves that exactly.
            match (source, edge) {
                (_, Edge::Annotation(ann)) => {
                    let ann_node =
                        walk_annotation(wf_lookup, act_lookup, ann, visiting_wf, visiting_action);
                    let (job_if, step_if) = match source {
                        SourceTier::JobStep { job, step } => {
                            (job.if_expr.as_deref(), step.if_expr.as_deref())
                        }
                        _ => (None, None),
                    };
                    children.push(maybe_guarded(ann_node, job_if, step_if));
                }
                (SourceTier::Job(job), Edge::CallsWorkflow(call)) => {
                    let node = match &call.workflow_ref {
                        WorkflowRef::Local(target) => walk_workflow(
                            wf_lookup,
                            act_lookup,
                            target,
                            visiting_wf,
                            visiting_action,
                        ),
                        WorkflowRef::External {
                            owner,
                            repo,
                            path,
                            gitref,
                        } => TraceNode::ExternalWorkflow {
                            owner: owner.clone(),
                            repo: repo.clone(),
                            path: path.clone(),
                            gitref: gitref.clone(),
                        },
                    };
                    children.push(maybe_guarded(node, job.if_expr.as_deref(), None));
                }
                (SourceTier::JobStep { job, step }, Edge::Uses(uses)) => {
                    if let Some(child) =
                        walk_uses_target(wf_lookup, act_lookup, uses, visiting_wf, visiting_action)
                    {
                        children.push(maybe_guarded(
                            child,
                            job.if_expr.as_deref(),
                            step.if_expr.as_deref(),
                        ));
                    }
                }
                // The walker never produces these (source, edge) combinations
                // for a workflow node; ignoring them keeps the match exhaustive
                // without panicking.
                (_, Edge::CallsWorkflow(_)) | (_, Edge::Uses(_)) => {}
            }
        }
    }
    visiting_wf.remove(&id.0);
    TraceNode::Workflow {
        id: id.clone(),
        children,
    }
}

/// Wraps `node` in a [`TraceNode::Guarded`] when at least one of the supplied
/// guard expressions is `Some`. Job-level and step-level conditions are
/// combined with logical AND (`(job_if) && (step_if)`), mirroring GitHub
/// Actions' short-circuit semantics where a step only runs when both its job
/// guard and its own guard evaluate to true. When only one side is present, it
/// is rendered verbatim without the redundant parentheses-and-AND wrapper.
fn maybe_guarded(node: TraceNode, job_if: Option<&str>, step_if: Option<&str>) -> TraceNode {
    let combined = match (job_if, step_if) {
        (Some(j), Some(s)) => Some(format!("({j}) && ({s})")),
        (Some(j), None) => Some(j.to_string()),
        (None, Some(s)) => Some(s.to_string()),
        (None, None) => None,
    };
    match combined {
        Some(expr) => TraceNode::Guarded {
            if_expr: expr,
            inner: Box::new(node),
        },
        None => node,
    }
}

fn walk_annotation(
    wf_lookup: &HashMap<&str, &Workflow>,
    act_lookup: &HashMap<&str, &LocalAction>,
    ann: &Annotation,
    visiting_wf: &mut HashSet<String>,
    visiting_action: &mut HashSet<String>,
) -> TraceNode {
    match &ann.resolution {
        AnnotationResolution::Resolved { target } => {
            let sub_children = if visiting_wf.contains(&target.0) {
                vec![TraceNode::Cycle(CycleTarget::Workflow(target.clone()))]
            } else {
                vec![walk_workflow(
                    wf_lookup,
                    act_lookup,
                    target,
                    visiting_wf,
                    visiting_action,
                )]
            };
            TraceNode::Annotated {
                verb: ann.verb,
                dangling: false,
                label: target.0.clone(),
                children: sub_children,
            }
        }
        AnnotationResolution::Dangling { raw_target, .. } => TraceNode::Annotated {
            verb: ann.verb,
            dangling: true,
            label: raw_target.clone(),
            children: Vec::new(),
        },
    }
}

fn walk_action(
    wf_lookup: &HashMap<&str, &Workflow>,
    act_lookup: &HashMap<&str, &LocalAction>,
    id: &ActionId,
    visiting_wf: &mut HashSet<String>,
    visiting_action: &mut HashSet<String>,
) -> TraceNode {
    if !visiting_action.insert(id.0.clone()) {
        return TraceNode::Cycle(CycleTarget::Action(id.clone()));
    }
    let mut children = Vec::new();
    if let Some(composite) = act_lookup.get(id.0.as_str()) {
        // Composite manifests never apply step-level guard wrapping (the IR's
        // composite-step `if:` is not surfaced as a trace guard, mirroring the
        // historical `walk_action` body). The walker still emits annotations
        // and step `uses:` in the same order as before.
        let edges: Vec<Edge<'_>> = {
            let mut buf = Vec::new();
            for_each_outgoing_edge(Node::Action(composite), |ctx| buf.push(ctx.edge));
            buf
        };
        for edge in edges {
            match edge {
                Edge::Annotation(ann) => {
                    children.push(walk_annotation(
                        wf_lookup,
                        act_lookup,
                        ann,
                        visiting_wf,
                        visiting_action,
                    ));
                }
                Edge::Uses(uses) => {
                    if let Some(child) =
                        walk_uses_target(wf_lookup, act_lookup, uses, visiting_wf, visiting_action)
                    {
                        children.push(child);
                    }
                }
                Edge::CallsWorkflow(_) => {
                    // The walker never emits CallsWorkflow for action nodes.
                }
            }
        }
    }
    visiting_action.remove(&id.0);
    TraceNode::Action {
        id: id.clone(),
        children,
    }
}

fn walk_uses_target(
    wf_lookup: &HashMap<&str, &Workflow>,
    act_lookup: &HashMap<&str, &LocalAction>,
    uses: &UsesRef,
    visiting_wf: &mut HashSet<String>,
    visiting_action: &mut HashSet<String>,
) -> Option<TraceNode> {
    match uses {
        UsesRef::LocalWorkflow(target) => Some(walk_workflow(
            wf_lookup,
            act_lookup,
            target,
            visiting_wf,
            visiting_action,
        )),
        UsesRef::LocalAction(target) => Some(walk_action(
            wf_lookup,
            act_lookup,
            target,
            visiting_wf,
            visiting_action,
        )),
        UsesRef::External {
            owner,
            repo,
            subpath,
            gitref,
        } => Some(TraceNode::External(ExternalActionRef {
            owner: owner.clone(),
            repo: repo.clone(),
            subpath: subpath.clone(),
            gitref: gitref.clone(),
        })),
        UsesRef::Docker(d) => Some(TraceNode::Docker(d.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::trace_render::{render_tree, TreeStyle};
    use crate::ui::Ui;
    use std::path::PathBuf;

    fn render_for_test(entries: &[TraceEntry]) -> String {
        render_tree(
            entries,
            None,
            &TreeStyle { unicode: true },
            &Ui::plain_for_test(),
        )
    }

    #[test]
    fn trigger_types_display_derives_three_variants() {
        // Explicit: types: [labeled, opened] on issues
        let explicit = TriggerSpec {
            types: Some(vec!["labeled".to_string(), "opened".to_string()]),
            ..TriggerSpec::bare(EventKind::Issues)
        };
        assert_eq!(
            TriggerTypesDisplay::from_trigger(&explicit),
            TriggerTypesDisplay::Explicit(vec!["labeled".to_string(), "opened".to_string()]),
        );

        // ImplicitAll: types omitted, no default subset (issues)
        let implicit_all = TriggerSpec::bare(EventKind::Issues);
        assert_eq!(
            TriggerTypesDisplay::from_trigger(&implicit_all),
            TriggerTypesDisplay::ImplicitAll,
        );

        // ImplicitDefault: types omitted, default subset (pull_request)
        let implicit_default = TriggerSpec::bare(EventKind::PullRequest);
        assert_eq!(
            TriggerTypesDisplay::from_trigger(&implicit_default),
            TriggerTypesDisplay::ImplicitDefault(vec![
                "opened".to_string(),
                "synchronize".to_string(),
                "reopened".to_string(),
            ]),
        );
    }

    #[test]
    fn trigger_match_sub_line_text() {
        // Explicit on issues
        let explicit = TriggerMatch {
            event: EventKind::Issues,
            types: TriggerTypesDisplay::Explicit(vec!["labeled".to_string(), "opened".to_string()]),
        };
        assert_eq!(
            explicit.sub_line_text(),
            Some("types: labeled, opened".to_string()),
        );

        // ImplicitAll on issues (event has activity types)
        let implicit_all_issues = TriggerMatch {
            event: EventKind::Issues,
            types: TriggerTypesDisplay::ImplicitAll,
        };
        assert_eq!(
            implicit_all_issues.sub_line_text(),
            Some("types: any".to_string()),
        );

        // ImplicitAll on repository_dispatch (custom event_type values)
        let implicit_all_repository_dispatch = TriggerMatch {
            event: EventKind::RepositoryDispatch,
            types: TriggerTypesDisplay::ImplicitAll,
        };
        assert_eq!(
            implicit_all_repository_dispatch.sub_line_text(),
            Some("types: any".to_string()),
        );

        // ImplicitDefault on pull_request
        let implicit_default = TriggerMatch {
            event: EventKind::PullRequest,
            types: TriggerTypesDisplay::ImplicitDefault(vec![
                "opened".to_string(),
                "synchronize".to_string(),
                "reopened".to_string(),
            ]),
        };
        assert_eq!(
            implicit_default.sub_line_text(),
            Some("types: opened, synchronize, reopened (default)".to_string()),
        );

        // ImplicitAll on push (event has no activity types — sub-line skipped)
        let push = TriggerMatch {
            event: EventKind::Push,
            types: TriggerTypesDisplay::ImplicitAll,
        };
        assert_eq!(push.sub_line_text(), None);
    }

    #[test]
    fn trace_json_entries_preserve_trigger_type_display() {
        let root = || TraceNode::Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            children: vec![],
        };
        let entries = trace_json_entries(&[
            TraceEntry {
                root: root(),
                trigger: TriggerMatch {
                    event: EventKind::Issues,
                    types: TriggerTypesDisplay::Explicit(vec!["opened".into(), "labeled".into()]),
                },
            },
            TraceEntry {
                root: root(),
                trigger: TriggerMatch {
                    event: EventKind::PullRequest,
                    types: TriggerTypesDisplay::ImplicitDefault(vec![
                        "opened".into(),
                        "synchronize".into(),
                        "reopened".into(),
                    ]),
                },
            },
            TraceEntry {
                root: root(),
                trigger: TriggerMatch {
                    event: EventKind::Issues,
                    types: TriggerTypesDisplay::ImplicitAll,
                },
            },
            TraceEntry {
                root: root(),
                trigger: TriggerMatch {
                    event: EventKind::Push,
                    types: TriggerTypesDisplay::ImplicitAll,
                },
            },
        ]);

        assert_eq!(
            entries[0].trigger.types,
            Some(TraceJsonTriggerTypes::Explicit {
                values: vec!["opened".into(), "labeled".into()]
            }),
        );
        assert_eq!(
            entries[1].trigger.types,
            Some(TraceJsonTriggerTypes::ImplicitDefault {
                values: vec!["opened".into(), "synchronize".into(), "reopened".into()]
            }),
        );
        assert_eq!(
            entries[2].trigger.types,
            Some(TraceJsonTriggerTypes::ImplicitAll)
        );
        assert_eq!(entries[3].trigger.types, None);
    }

    #[test]
    fn trace_json_entries_convert_all_node_variants() {
        let entries = trace_json_entries(&[TraceEntry {
            root: TraceNode::Workflow {
                id: WorkflowId(".github/workflows/ci.yml".into()),
                children: vec![TraceNode::Action {
                    id: ActionId(".github/actions/setup".into()),
                    children: vec![
                        TraceNode::External(ExternalActionRef {
                            owner: "actions".into(),
                            repo: "checkout".into(),
                            subpath: Some("dist".into()),
                            gitref: "v4".into(),
                        }),
                        TraceNode::ExternalWorkflow {
                            owner: "acme".into(),
                            repo: "automation".into(),
                            path: ".github/workflows/deploy.yml".into(),
                            gitref: "main".into(),
                        },
                        TraceNode::Docker(DockerRef {
                            host: Some("ghcr.io".into()),
                            image: "acme/build".into(),
                            tag: Some("1.2.3".into()),
                        }),
                        TraceNode::Annotated {
                            verb: AnnotationVerb::Dispatches,
                            dangling: false,
                            label: ".github/workflows/deploy.yml".into(),
                            children: vec![TraceNode::Cycle(CycleTarget::Workflow(WorkflowId(
                                ".github/workflows/ci.yml".into(),
                            )))],
                        },
                        TraceNode::Cycle(CycleTarget::Action(ActionId(
                            ".github/actions/setup".into(),
                        ))),
                        TraceNode::Guarded {
                            if_expr: "github.ref == 'refs/heads/main'".into(),
                            inner: Box::new(TraceNode::External(ExternalActionRef {
                                owner: "docker".into(),
                                repo: "login-action".into(),
                                subpath: None,
                                gitref: "v3".into(),
                            })),
                        },
                    ],
                }],
            },
            trigger: TriggerMatch {
                event: EventKind::RepositoryDispatch,
                types: TriggerTypesDisplay::ImplicitAll,
            },
        }]);

        assert_eq!(
            entries[0],
            TraceJsonEntry {
                root: TraceJsonNode::Workflow {
                    id: ".github/workflows/ci.yml".into(),
                    children: vec![TraceJsonNode::Action {
                        id: ".github/actions/setup".into(),
                        children: vec![
                            TraceJsonNode::ExternalAction {
                                owner: "actions".into(),
                                repo: "checkout".into(),
                                subpath: Some("dist".into()),
                                gitref: "v4".into(),
                            },
                            TraceJsonNode::ExternalWorkflow {
                                owner: "acme".into(),
                                repo: "automation".into(),
                                path: ".github/workflows/deploy.yml".into(),
                                gitref: "main".into(),
                            },
                            TraceJsonNode::Docker {
                                image: "ghcr.io/acme/build:1.2.3".into(),
                            },
                            TraceJsonNode::Annotated {
                                verb: AnnotationVerb::Dispatches,
                                dangling: false,
                                label: ".github/workflows/deploy.yml".into(),
                                children: vec![TraceJsonNode::Cycle {
                                    target_kind: "workflow",
                                    target: ".github/workflows/ci.yml".into(),
                                }],
                            },
                            TraceJsonNode::Cycle {
                                target_kind: "action",
                                target: ".github/actions/setup".into(),
                            },
                            TraceJsonNode::Guarded {
                                if_expr: "github.ref == 'refs/heads/main'".into(),
                                inner: Box::new(TraceJsonNode::ExternalAction {
                                    owner: "docker".into(),
                                    repo: "login-action".into(),
                                    subpath: None,
                                    gitref: "v3".into(),
                                }),
                            },
                        ],
                    }],
                },
                trigger: TraceJsonTrigger {
                    event: "repository_dispatch".into(),
                    types: Some(TraceJsonTriggerTypes::ImplicitAll),
                },
            },
        );
    }

    fn wf_with_annotations(id: &str, anns: Vec<Annotation>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: anns,
        }
    }

    fn empty_target_workflow(id: &str) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
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
        }
    }

    #[test]
    fn trace_emits_annotated_edge_to_resolved_target() {
        let wf = wf_with_annotations(
            ".github/workflows/ci.yml",
            vec![Annotation {
                verb: AnnotationVerb::Dispatches,
                resolution: AnnotationResolution::Resolved {
                    target: WorkflowId(".github/workflows/build.yml".into()),
                },
                source_line: 3,
            }],
        );
        let target = empty_target_workflow(".github/workflows/build.yml");
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![wf, target],
            actions: vec![],
            external_actions: vec![],
        };
        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        assert_eq!(roots.len(), 1);
        let rendered = render_for_test(&roots);
        assert!(
            rendered.contains(".github/workflows/build.yml  [ann]  via dispatches"),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn trace_handles_workflow_call_cycle_without_infinite_loop() {
        // A.job -> calls B; B.job -> calls A. The recursion must stop on the
        // second visit via `visiting_wf` cycle guard rather than recurse forever.
        fn wf_calling(id: &str, target_id: &str, entry: bool) -> Workflow {
            let triggers = if entry {
                vec![TriggerSpec::bare(EventKind::Push)]
            } else {
                vec![TriggerSpec::bare(EventKind::WorkflowCall)]
            };
            Workflow {
                id: WorkflowId(id.into()),
                source: SourcePos {
                    file: PathBuf::new(),
                    line: Some(1),
                },
                name: None,
                run_name: None,
                triggers,
                jobs: vec![Job {
                    id: JobId("j".into()),
                    workflow: WorkflowId(id.into()),
                    needs: vec![],
                    permissions: None,
                    steps: vec![],
                    calls_workflow: Some(CallsWorkflow {
                        workflow_ref: WorkflowRef::Local(WorkflowId(target_id.into())),
                        with: Default::default(),
                        secrets: SecretsPass::None,
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
                }],
                permissions: None,
                defaults: None,
                env: Default::default(),
                concurrency: None,
                annotations: Vec::new(),
            }
        }

        let a = wf_calling(".github/workflows/a.yml", ".github/workflows/b.yml", true);
        let b = wf_calling(".github/workflows/b.yml", ".github/workflows/a.yml", false);
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![a, b],
            actions: vec![],
            external_actions: vec![],
        };

        // Should return in finite time. trace(...) returns one tree per
        // entry-point matching `event`. A is the only entry-point.
        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        assert_eq!(roots.len(), 1);
        let rendered = render_for_test(&roots);
        // The cycle guard emits a Cycle node for the second occurrence of A
        // inside B's subtree, distinguishing it from a real leaf.
        assert!(
            rendered.contains(".github/workflows/a.yml")
                && rendered.contains(".github/workflows/b.yml"),
            "rendered: {rendered}"
        );
        assert!(
            rendered.contains(".github/workflows/a.yml  [cyc]  guard"),
            "expected cycle marker for A inside B's subtree, rendered: {rendered}"
        );
    }

    #[test]
    fn trace_emits_cycle_node_for_composite_self_call() {
        // Entry-point workflow uses composite A; A's only step `uses:` A itself.
        // walk_action's cycle guard must emit a Cycle node, not an empty leaf.
        let composite_id = ".github/actions/self-recurse";
        let entry = Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![Job {
                id: JobId("j".into()),
                workflow: WorkflowId(".github/workflows/ci.yml".into()),
                needs: vec![],
                permissions: None,
                steps: vec![Step {
                    index: 0,
                    id: None,
                    name: None,
                    uses: Some(UsesRef::LocalAction(ActionId(composite_id.into()))),
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
            }],
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: Vec::new(),
        };
        let composite = LocalAction {
            id: ActionId(composite_id.into()),
            source: SourcePos {
                file: PathBuf::new(),
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
                uses: Some(UsesRef::LocalAction(ActionId(composite_id.into()))),
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
            annotations: Vec::new(),
        };
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![entry],
            actions: vec![composite],
            external_actions: vec![],
        };

        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        assert_eq!(roots.len(), 1);

        // Structural assertion: drill into roots[0].jobs[0].steps[0] -> Action(A)
        // -> children[0] should be Cycle(Action(A)).
        let TraceNode::Workflow { children, .. } = &roots[0].root else {
            panic!("root not Workflow: {:?}", roots[0]);
        };
        assert_eq!(
            children.len(),
            1,
            "entry workflow expected one child action"
        );
        let TraceNode::Action {
            id: outer_id,
            children: outer_children,
        } = &children[0]
        else {
            panic!("expected Action node, got {:?}", children[0]);
        };
        assert_eq!(outer_id.0, composite_id);
        assert_eq!(
            outer_children.len(),
            1,
            "composite should have one cycle child"
        );
        match &outer_children[0] {
            TraceNode::Cycle(CycleTarget::Action(id)) => {
                assert_eq!(id.0, composite_id);
            }
            other => panic!("expected Cycle(Action(...)), got {other:?}"),
        }

        let rendered = render_for_test(&roots);
        assert!(
            rendered.contains(&format!("{composite_id}  [cyc]  guard")),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn trace_emits_cycle_node_for_annotation_self_reference() {
        // wf A has an annotation whose target is A itself. walk_annotation's
        // cycle-guard branch must emit Annotated { children: [Cycle(Workflow(A))] },
        // not Annotated { children: [] }.
        let id = ".github/workflows/self.yml";
        let wf = wf_with_annotations(
            id,
            vec![Annotation {
                verb: AnnotationVerb::Dispatches,
                resolution: AnnotationResolution::Resolved {
                    target: WorkflowId(id.into()),
                },
                source_line: 3,
            }],
        );
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![wf],
            actions: vec![],
            external_actions: vec![],
        };
        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        assert_eq!(roots.len(), 1);

        let TraceNode::Workflow { children, .. } = &roots[0].root else {
            panic!("root not Workflow: {:?}", roots[0]);
        };
        assert_eq!(children.len(), 1, "expected one annotated child");
        let TraceNode::Annotated {
            children: ann_children,
            ..
        } = &children[0]
        else {
            panic!("expected Annotated node, got {:?}", children[0]);
        };
        assert_eq!(
            ann_children.len(),
            1,
            "annotation cycle branch should emit one Cycle child"
        );
        match &ann_children[0] {
            TraceNode::Cycle(CycleTarget::Workflow(target_id)) => {
                assert_eq!(target_id.0, id);
            }
            other => panic!("expected Cycle(Workflow(...)), got {other:?}"),
        }

        let rendered = render_for_test(&roots);
        assert!(
            rendered.contains(&format!("{id}  [cyc]  guard")),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn trace_emits_external_workflow_node_for_cross_repo_workflow_call() {
        // leaf.yml job uses WorkflowRef::External — must emit TraceNode::ExternalWorkflow,
        // NOT TraceNode::External(ExternalActionRef). This ensures the render layer
        // preserves the semantic distinction between reusable workflows and actions.
        let caller = Workflow {
            id: WorkflowId(".github/workflows/caller.yml".into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![Job {
                id: JobId("call".into()),
                workflow: WorkflowId(".github/workflows/caller.yml".into()),
                needs: vec![],
                permissions: None,
                steps: vec![],
                calls_workflow: Some(CallsWorkflow {
                    workflow_ref: WorkflowRef::External {
                        owner: "acme".into(),
                        repo: "shared".into(),
                        path: ".github/workflows/deploy.yml".into(),
                        gitref: "v1".into(),
                    },
                    with: Default::default(),
                    secrets: SecretsPass::Inherit,
                }),
                outputs: Default::default(),
                source: SourcePos {
                    file: PathBuf::new(),
                    line: None,
                },
                runs_on: None,
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
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![caller],
            actions: vec![],
            external_actions: vec![],
        };
        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        assert_eq!(roots.len(), 1);
        let TraceNode::Workflow { children, .. } = &roots[0].root else {
            panic!("root not Workflow: {:?}", roots[0]);
        };
        assert_eq!(children.len(), 1, "expected one child");
        match &children[0] {
            TraceNode::ExternalWorkflow {
                owner,
                repo,
                path,
                gitref,
            } => {
                assert_eq!(owner, "acme");
                assert_eq!(repo, "shared");
                assert_eq!(path, ".github/workflows/deploy.yml");
                assert_eq!(gitref, "v1");
            }
            other => panic!("expected ExternalWorkflow, got {other:?}"),
        }
        let rendered = render_for_test(&roots);
        assert!(
            rendered.contains("acme/shared/.github/workflows/deploy.yml  [ext-wf]  @v1"),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn trace_emits_dangling_label_for_unresolved_annotation() {
        let wf = wf_with_annotations(
            ".github/workflows/ci.yml",
            vec![Annotation {
                verb: AnnotationVerb::Triggers,
                resolution: AnnotationResolution::Dangling {
                    raw_target: "missing.yml".into(),
                    reason: "target must live under .github/workflows/".into(),
                },
                source_line: 3,
            }],
        );
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![wf],
            actions: vec![],
            external_actions: vec![],
        };
        let rendered = render_for_test(&trace(&ir, "push", &[], &[], &[], &[]));
        assert!(
            rendered.contains("missing.yml  [ann]  via triggers · dangling"),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn trace_marks_guarded_edge_when_job_has_if_expr() {
        // Entry workflow has a job with `if_expr` that calls a reusable workflow.
        // The resulting TraceNode::Workflow child should be wrapped in Guarded,
        // and the renderer should append "(if: <expr>)" to its label.
        let callee_id = ".github/workflows/deploy.yml";
        let expr = "github.event_name == 'push'";
        let caller = Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![Job {
                id: JobId("deploy".into()),
                workflow: WorkflowId(".github/workflows/ci.yml".into()),
                needs: vec![],
                permissions: None,
                steps: vec![],
                calls_workflow: Some(CallsWorkflow {
                    workflow_ref: WorkflowRef::Local(WorkflowId(callee_id.into())),
                    with: Default::default(),
                    secrets: SecretsPass::None,
                }),
                outputs: Default::default(),
                source: SourcePos {
                    file: PathBuf::new(),
                    line: None,
                },
                runs_on: None,
                environment: None,
                if_expr: Some(expr.into()),
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
        let callee = Workflow {
            id: WorkflowId(callee_id.into()),
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
            workflows: vec![caller, callee],
            actions: vec![],
            external_actions: vec![],
        };

        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        assert_eq!(roots.len(), 1);

        // The root workflow's child should be a Guarded wrapping Workflow(callee).
        let TraceNode::Workflow { children, .. } = &roots[0].root else {
            panic!("root not Workflow: {:?}", roots[0]);
        };
        assert_eq!(children.len(), 1, "expected one child");
        match &children[0] {
            TraceNode::Guarded {
                if_expr: guard_expr,
                inner,
            } => {
                assert_eq!(guard_expr, expr);
                match inner.as_ref() {
                    TraceNode::Workflow { id, .. } => assert_eq!(id.0, callee_id),
                    other => panic!("expected Workflow inside Guarded, got {:?}", other),
                }
            }
            other => panic!("expected Guarded child, got {:?}", other),
        }

        let rendered = render_for_test(&roots);
        // R4: `if:` is rendered as a synthetic `╰─ if: <expr>` child line under
        // the guarded workflow, not as a `· if:` suffix.
        assert!(
            rendered.contains(&format!("{callee_id}  [wf]")),
            "rendered: {rendered}"
        );
        assert!(
            rendered.contains(&format!("╰─ if: {expr}")),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn trace_emits_docker_leaf_for_docker_step() {
        // Entry-point workflow with one step that uses a Docker action.
        // walk_uses must emit TraceNode::Docker (a leaf — no further traversal).
        let entry = Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![Job {
                id: JobId("j".into()),
                workflow: WorkflowId(".github/workflows/ci.yml".into()),
                needs: vec![],
                permissions: None,
                steps: vec![Step {
                    index: 0,
                    id: None,
                    name: None,
                    uses: Some(UsesRef::Docker(DockerRef {
                        host: None,
                        image: "alpine".into(),
                        tag: Some("3.8".into()),
                    })),
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
                outputs: Default::default(),
                source: SourcePos {
                    file: PathBuf::new(),
                    line: None,
                },
                runs_on: None,
                environment: None,
                if_expr: None,
                strategy: None,
                concurrency: None,
                defaults: None,
                env: Default::default(),
                container: None,
                services: Default::default(),
                annotations: Vec::new(),
            }],
            permissions: None,
            concurrency: None,
            defaults: None,
            env: Default::default(),
            annotations: Vec::new(),
        };
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![entry],
            actions: vec![],
            external_actions: vec![],
        };

        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        assert_eq!(roots.len(), 1);

        // Structural assertion: the workflow should have one Docker child.
        let TraceNode::Workflow { children, .. } = &roots[0].root else {
            panic!("root not Workflow: {:?}", roots[0]);
        };
        assert_eq!(children.len(), 1, "expected one Docker child");
        match &children[0] {
            TraceNode::Docker(d) => {
                assert_eq!(d.host, None);
                assert_eq!(d.image, "alpine");
                assert_eq!(d.tag.as_deref(), Some("3.8"));
            }
            other => panic!("expected TraceNode::Docker, got {other:?}"),
        }

        // Rendered output must include a docker label.
        let rendered = render_for_test(&roots);
        assert!(
            rendered.contains("alpine  [docker]  :3.8"),
            "rendered: {rendered}"
        );
    }

    /// Builds a single-job, single-step workflow whose step `uses:` an external
    /// action. `job_if` and `step_if` are passed verbatim into `Job.if_expr`
    /// and `Step.if_expr` respectively.
    fn step_uses_workflow(job_if: Option<&str>, step_if: Option<&str>) -> Workflow {
        Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![Job {
                id: JobId("j".into()),
                workflow: WorkflowId(".github/workflows/ci.yml".into()),
                needs: vec![],
                permissions: None,
                steps: vec![Step {
                    index: 0,
                    id: None,
                    name: None,
                    uses: Some(UsesRef::External {
                        owner: "actions".into(),
                        repo: "checkout".into(),
                        subpath: None,
                        gitref: "v4".into(),
                    }),
                    run: None,
                    if_expr: step_if.map(|s| s.into()),
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
                if_expr: job_if.map(|s| s.into()),
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
        }
    }

    #[test]
    fn trace_marks_guarded_edge_when_step_has_only_step_if() {
        // No job-level guard, but the step has `if:` — the edge must still be
        // wrapped in Guarded with the step expression as-is.
        let step_expr = "matrix.os == 'ubuntu-latest'";
        let wf = step_uses_workflow(None, Some(step_expr));
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![wf],
            actions: vec![],
            external_actions: vec![],
        };

        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        let TraceNode::Workflow { children, .. } = &roots[0].root else {
            panic!("root not Workflow: {:?}", roots[0]);
        };
        assert_eq!(children.len(), 1);
        match &children[0] {
            TraceNode::Guarded { if_expr, inner } => {
                assert_eq!(
                    if_expr, step_expr,
                    "step-only guard must pass through verbatim"
                );
                assert!(matches!(inner.as_ref(), TraceNode::External(_)));
            }
            other => panic!("expected Guarded, got {other:?}"),
        }
    }

    #[test]
    fn trace_combines_job_and_step_if_with_logical_and() {
        // Both job-level and step-level `if:` present — the rendered guard
        // must combine them as `(job_if) && (step_if)` to mirror GitHub
        // Actions' short-circuit AND semantics.
        let job_expr = "github.event_name == 'push'";
        let step_expr = "runner.os == 'Linux'";
        let wf = step_uses_workflow(Some(job_expr), Some(step_expr));
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![wf],
            actions: vec![],
            external_actions: vec![],
        };

        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        let TraceNode::Workflow { children, .. } = &roots[0].root else {
            panic!("root not Workflow: {:?}", roots[0]);
        };
        assert_eq!(children.len(), 1);
        match &children[0] {
            TraceNode::Guarded { if_expr, .. } => {
                assert_eq!(if_expr, &format!("({job_expr}) && ({step_expr})"));
            }
            other => panic!("expected Guarded, got {other:?}"),
        }

        let rendered = render_for_test(&roots);
        // R4: combined `if:` renders on its own synthetic child line.
        assert!(
            rendered.contains(&format!("╰─ if: ({job_expr}) && ({step_expr})")),
            "rendered: {rendered}"
        );
    }

    #[test]
    fn trace_step_only_if_does_not_change_job_only_rendering() {
        // Regression guard: when ONLY job-level `if:` is set (no step-level),
        // the rendered expression must remain bare — no extra parentheses or
        // `&&` artifacts from the combiner.
        let job_expr = "github.event_name == 'push'";
        let wf = step_uses_workflow(Some(job_expr), None);
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![wf],
            actions: vec![],
            external_actions: vec![],
        };

        let roots = trace(&ir, "push", &[], &[], &[], &[]);
        let TraceNode::Workflow { children, .. } = &roots[0].root else {
            panic!("root not Workflow: {:?}", roots[0]);
        };
        assert_eq!(children.len(), 1);
        match &children[0] {
            TraceNode::Guarded { if_expr, .. } => {
                assert_eq!(if_expr, job_expr, "job-only guard must not gain wrapping");
            }
            other => panic!("expected Guarded, got {other:?}"),
        }
    }
}
