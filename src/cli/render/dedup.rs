use crate::cache;
use crate::query::{self, dedup::DedupCluster};
use crate::ui::{self, Status, Ui};
use anyhow::Result;
use globset::GlobSet;

use crate::cli::{build_or_load, ReportFormat};

pub(in crate::cli) fn run(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    threshold: f32,
    format: &ReportFormat,
    ui: &Ui,
) -> Result<()> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    let clusters = query::dedup::dedup(&ir, threshold);
    match format {
        ReportFormat::Markdown => {
            println!("### Near-duplicate workflows");
            println!();
            if clusters.is_empty() {
                println!(
                    "No near-duplicate workflow clusters found at threshold `{:.2}`.",
                    threshold
                );
            } else {
                println!(
                    "{} found at threshold `{:.2}`.",
                    ui::plural(clusters.len(), "workflow cluster", "workflow clusters"),
                    threshold
                );
                println!();
                println!("| Cluster | Representative | Members | Triggers differ |");
                println!("|---|---|---:|---|");
                // `cluster_index` is 0-based to match the JSON output (and the
                // original dogfood `bash + jq` Markdown). The Text renderer below
                // shows it 1-based for human readability; do not align them.
                for c in &clusters {
                    println!(
                        "| #{} | `{}` | {} | {} |",
                        c.cluster_index,
                        c.representative.0,
                        c.members.len() + 1,
                        c.triggers_differ
                    );
                }
            }
        }
        ReportFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&clusters)?);
        }
        ReportFormat::Text => {
            if clusters.is_empty() {
                println!(
                    "{}",
                    ui.status_header(
                        "dedup",
                        Status::Clean,
                        "no near-duplicate clusters",
                        &[format!("threshold={threshold:.2}")],
                    )
                );
            } else {
                render_clusters_text(&clusters, threshold, ui);
            }
        }
    }
    Ok(())
}

fn render_clusters_text(clusters: &[DedupCluster], threshold: f32, ui: &Ui) {
    println!(
        "{}",
        ui.status_header(
            "dedup",
            Status::Found,
            ui::plural(clusters.len(), "workflow cluster", "workflow clusters"),
            &[format!("threshold={threshold:.2}")],
        )
    );
    for c in clusters.iter() {
        println!();
        let total = c.members.len() + 1;
        println!(
            "{}",
            ui.section(&format!("Cluster {}", c.cluster_index + 1))
        );
        print!(
            "{}",
            ui.table(
                &["metric", "value"],
                &[
                    vec!["workflows".into(), total.to_string()],
                    vec!["representative".into(), c.representative.0.clone()],
                    vec![
                        "triggers differ".into(),
                        if c.triggers_differ { "yes" } else { "no" }.into(),
                    ],
                ],
            )
        );
        if !c.members.is_empty() {
            println!();
            println!("{}", ui.section("Members"));
            let rows: Vec<Vec<String>> = c
                .members
                .iter()
                .map(|m| {
                    vec![
                        m.workflow.0.clone(),
                        format!("{:.2}", m.similarity_to_representative),
                    ]
                })
                .collect();
            print!("{}", ui.table(&["member", "similarity"], &rows));
        }
        println!();
        println!("{}", ui.section("Uses"));
        print!(
            "{}",
            ui.table(
                &["label", "value"],
                &[
                    vec!["common".into(), format_uses_list(&c.common_uses)],
                    vec!["divergent".into(), format_uses_list(&c.divergent_uses)],
                ],
            )
        );
    }
}

fn format_uses_list(items: &[String]) -> String {
    if items.is_empty() {
        "(none)".into()
    } else {
        items.join(", ")
    }
}
