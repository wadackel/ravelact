use crate::cache;
use crate::ir::{ActionId, AnnotationVerb, Ir, WorkflowId};
use crate::markdown;
use crate::query::{
    self,
    callers::{AnnotationAnchor, CallerHit, CompositeAnnotationAnchor},
    Target,
};
use crate::ui::{self, Status, Ui};
use anyhow::Result;
use globset::GlobSet;

use crate::cli::render::findings_overlay::{self, NodeKey};
use crate::cli::{build_or_load, stdin_input, FindingsArgs, ReportFormat};

const TABLE_HEADERS: [&str; 3] = ["kind", "location", "detail"];

pub(in crate::cli) fn run(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    targets: &[String],
    format: &ReportFormat,
    findings: &FindingsArgs,
    ui: &Ui,
) -> Result<()> {
    let inputs = stdin_input::collect(targets)?;
    let ir = build_or_load(root, cache_mode, excludes)?;
    let per_target: Vec<(String, Vec<CallerHit>)> = inputs
        .into_iter()
        .map(|t| {
            let hits = query::callers::callers(&ir, &Target::from_user_input(&t));
            (t, hits)
        })
        .collect();

    // External-finding overlay: blast radius = each queried target plus its
    // caller nodes. Empty when `--findings` is absent.
    let enriched = if findings.findings.is_empty() {
        Vec::new()
    } else {
        findings_overlay::load_enriched(&ir, &findings.findings)?
    };
    let grouped = findings_overlay::group_by_node(&enriched);
    let combined_scope: Vec<(NodeKey, String)> = per_target
        .iter()
        .flat_map(|(target, hits)| callers_scope(target, hits))
        .collect();

    match format {
        ReportFormat::Markdown => {
            println!("### Callers");
            println!();
            let total_hits: usize = per_target.iter().map(|(_, h)| h.len()).sum();
            if total_hits == 0 {
                println!(
                    "No callers found across {}.",
                    ui::plural(per_target.len(), "target", "targets")
                );
            } else {
                println!(
                    "{} found across {}.",
                    ui::plural(total_hits, "caller", "callers"),
                    ui::plural(per_target.len(), "target", "targets")
                );
                println!();
                println!("| Target | Kind | Location | Detail |");
                println!("|---|---|---|---|");
                for (target, hits) in &per_target {
                    if hits.is_empty() {
                        println!("| {} | - | - | no callers |", markdown::code_cell(target));
                        continue;
                    }
                    for hit in hits {
                        let row = format_caller_row(hit, &ir);
                        println!(
                            "| {} | {} | {} | {} |",
                            markdown::code_cell(target),
                            markdown::code_cell(&row[0]),
                            markdown::code_cell(&row[1]),
                            markdown::code_cell(&row[2])
                        );
                    }
                }
            }
            if findings.show_findings {
                let body = findings_overlay::render_scoped_findings(
                    &grouped,
                    &combined_scope,
                    findings.show_priority,
                );
                if !body.is_empty() {
                    println!();
                    println!("#### Findings (blast radius)");
                    println!();
                    print!("{body}");
                }
            }
        }
        ReportFormat::Json => {
            let payload: Vec<serde_json::Value> = per_target
                .iter()
                .map(|(t, hits)| {
                    let mut obj = serde_json::json!({ "target": t, "hits": hits });
                    if !findings.findings.is_empty() {
                        let scope = callers_scope(t, hits);
                        let scoped = findings_overlay::scoped_findings(&grouped, &scope);
                        obj["findings"] = findings_overlay::findings_json(&scoped)?;
                    }
                    Ok::<serde_json::Value, anyhow::Error>(obj)
                })
                .collect::<Result<Vec<_>>>()?;
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        ReportFormat::Text => {
            let total_hits: usize = per_target.iter().map(|(_, h)| h.len()).sum();
            let findings_body = if findings.show_findings {
                findings_overlay::render_scoped_findings(
                    &grouped,
                    &combined_scope,
                    findings.show_priority,
                )
            } else {
                String::new()
            };
            if total_hits == 0 {
                println!(
                    "{}",
                    ui.status_header(
                        "callers",
                        Status::Clean,
                        "no callers found",
                        &[format!("{} targets", per_target.len())],
                    )
                );
            } else if per_target.len() == 1 {
                let (input, hits) = &per_target[0];
                println!(
                    "{}",
                    ui.status_header(
                        "callers",
                        Status::Found,
                        format!(
                            "{} for {input}",
                            ui::plural(hits.len(), "caller", "callers")
                        ),
                        &[],
                    )
                );
                println!();
                println!("{}", ui.section("Callers"));
                let rows: Vec<Vec<String>> =
                    hits.iter().map(|hit| format_caller_row(hit, &ir)).collect();
                print!("{}", ui.table(&TABLE_HEADERS, &rows));
            } else {
                println!(
                    "{}",
                    ui.status_header(
                        "callers",
                        Status::Found,
                        format!(
                            "{} queried",
                            ui::plural(per_target.len(), "target", "targets")
                        ),
                        &[],
                    )
                );
                for (input, hits) in &per_target {
                    println!();
                    println!("{}", ui.section_path(input));
                    if hits.is_empty() {
                        println!("  no callers");
                        continue;
                    }
                    println!("  {}", ui::plural(hits.len(), "caller", "callers"));
                    let rows: Vec<Vec<String>> =
                        hits.iter().map(|hit| format_caller_row(hit, &ir)).collect();
                    print!("{}", ui.table(&TABLE_HEADERS, &rows));
                }
            }
            if !findings_body.is_empty() {
                println!();
                println!("{}", ui.section("Findings (blast radius)"));
                print!("{findings_body}");
            }
        }
    }
    Ok(())
}

/// Finding-overlay node scope for one `callers` target: the queried target node
/// plus every caller hit's node (the blast radius).
fn callers_scope(target: &str, hits: &[CallerHit]) -> Vec<(NodeKey, String)> {
    let mut scope: Vec<(NodeKey, String)> = Vec::new();
    match Target::from_user_input(target) {
        Target::Workflow(id) => {
            let display = id.0.clone();
            scope.push((NodeKey::Workflow(id), display));
        }
        Target::Action(id) => {
            let display = id.0.clone();
            scope.push((NodeKey::Action(id), display));
        }
    }
    for hit in hits {
        match hit {
            CallerHit::JobCall { workflow, .. }
            | CallerHit::Step { workflow, .. }
            | CallerHit::Annotated { workflow, .. } => {
                scope.push((NodeKey::Workflow(workflow.clone()), workflow.0.clone()));
            }
            CallerHit::CompositeStep { action, .. }
            | CallerHit::AnnotatedComposite { action, .. } => {
                scope.push((NodeKey::Action(action.clone()), action.0.clone()));
            }
        }
    }
    scope
}

fn format_caller_row(hit: &CallerHit, ir: &Ir) -> Vec<String> {
    match hit {
        CallerHit::JobCall { workflow, job } => vec![
            "job-call".into(),
            workflow.0.clone(),
            format!("{job}::_jobcall"),
        ],
        CallerHit::Step {
            workflow,
            job,
            step_index,
        } => {
            let detail = match workflow_step_name(ir, workflow, job, *step_index) {
                Some(name) => format!("{job}:{step_index}  name={}", json_string(name)),
                None => format!("{job}:{step_index}"),
            };
            vec!["step".into(), workflow.0.clone(), detail]
        }
        CallerHit::CompositeStep { action, step_index } => {
            let detail = match composite_step_name(ir, action, *step_index) {
                Some(name) => format!("_composite:{step_index}  name={}", json_string(name)),
                None => format!("_composite:{step_index}"),
            };
            vec!["composite-step".into(), action.0.clone(), detail]
        }
        CallerHit::Annotated {
            workflow,
            anchor,
            verb,
        } => {
            let verb_str = match verb {
                AnnotationVerb::Dispatches => "dispatches",
                AnnotationVerb::Triggers => "triggers",
            };
            let anchor_str = match anchor {
                AnnotationAnchor::Workflow => "_workflow".to_string(),
                AnnotationAnchor::Job { job } => format!("{job}:_job"),
                AnnotationAnchor::Step { job, step_index } => format!("{job}:{step_index}"),
            };
            let name = match anchor {
                AnnotationAnchor::Step { job, step_index } => {
                    workflow_step_name(ir, workflow, job, *step_index)
                        .map(|name| format!("  name={}", json_string(name)))
                        .unwrap_or_default()
                }
                _ => String::new(),
            };
            vec![
                "annotated".into(),
                workflow.0.clone(),
                format!("{anchor_str} via {verb_str}{name}"),
            ]
        }
        CallerHit::AnnotatedComposite {
            action,
            anchor,
            verb,
        } => {
            let verb_str = match verb {
                AnnotationVerb::Dispatches => "dispatches",
                AnnotationVerb::Triggers => "triggers",
            };
            let anchor_str = match anchor {
                CompositeAnnotationAnchor::Action => "_action".to_string(),
                CompositeAnnotationAnchor::Step { step_index } => {
                    format!("_composite:{step_index}")
                }
            };
            let name = match anchor {
                CompositeAnnotationAnchor::Step { step_index } => {
                    composite_step_name(ir, action, *step_index)
                        .map(|name| format!("  name={}", json_string(name)))
                        .unwrap_or_default()
                }
                CompositeAnnotationAnchor::Action => String::new(),
            };
            vec![
                "annotated-composite".into(),
                action.0.clone(),
                format!("{anchor_str} via {verb_str}{name}"),
            ]
        }
    }
}

fn workflow_step_name<'a>(
    ir: &'a Ir,
    workflow: &WorkflowId,
    job: &str,
    idx: usize,
) -> Option<&'a str> {
    ir.workflows
        .iter()
        .find(|w| w.id == *workflow)?
        .jobs
        .iter()
        .find(|j| j.id.0 == job)?
        .steps
        .get(idx)?
        .name
        .as_deref()
}

fn composite_step_name<'a>(ir: &'a Ir, action: &ActionId, idx: usize) -> Option<&'a str> {
    ir.actions
        .iter()
        .find(|c| c.id == *action)?
        .steps
        .get(idx)?
        .name
        .as_deref()
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).expect("&str to JSON cannot fail")
}
