use crate::cache;
use crate::suggest_extract;
use crate::ui::{self, Status, Ui};
use anyhow::Result;
use globset::GlobSet;

use crate::cli::{build_or_load, ReportFormat};

fn extract_metadata(min_length: usize, min_occurrences: usize) -> Vec<String> {
    vec![
        format!("min-length={min_length}"),
        format!("min-occurrences={min_occurrences}"),
    ]
}

pub(in crate::cli) fn run(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    min_length: usize,
    min_occurrences: usize,
    format: &ReportFormat,
    ui: &Ui,
) -> Result<()> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    let candidates = suggest_extract::find_candidates(&ir, min_length, min_occurrences);
    match format {
        ReportFormat::Markdown => {
            println!("### Extract");
            println!();
            if candidates.is_empty() {
                println!("No extraction candidates found.");
            } else {
                println!(
                    "{} found.",
                    ui::plural(
                        candidates.len(),
                        "extraction candidate",
                        "extraction candidates",
                    )
                );
                println!();
                println!("| Score | Length | Occurrences | First citation |");
                println!("|---:|---:|---:|---|");
                for c in &candidates {
                    // `find_candidates` only emits a Candidate when occurrences
                    // meet `min_occurrences` (>= 2 by CLI default), so [0] is
                    // safe.
                    let first = &c.occurrences[0];
                    println!(
                        "| {} | {} | {} | `{}:{}..{}` |",
                        c.score,
                        c.length,
                        c.occurrences.len(),
                        first.container,
                        first.start,
                        first.end
                    );
                }
            }
        }
        ReportFormat::Json => {
            println!("{}", suggest_extract::render_json(&candidates)?);
        }
        ReportFormat::Text => {
            render_text(&candidates, min_length, min_occurrences, ui);
        }
    }
    Ok(())
}

fn render_text(
    candidates: &[suggest_extract::Candidate],
    min_length: usize,
    min_occurrences: usize,
    ui: &Ui,
) {
    let metadata = extract_metadata(min_length, min_occurrences);
    if candidates.is_empty() {
        println!(
            "{}",
            ui.status_header(
                "extract",
                Status::Clean,
                "no extraction candidates",
                &metadata
            )
        );
        return;
    }
    println!(
        "{}",
        ui.status_header(
            "extract",
            Status::Found,
            ui::plural(
                candidates.len(),
                "extraction candidate",
                "extraction candidates",
            ),
            &metadata,
        )
    );
    for (i, c) in candidates.iter().enumerate() {
        println!();
        println!("{}", ui.section(&format!("Candidate {}", i + 1)));
        print!(
            "{}",
            ui.table(
                &["score", "length", "occurrences"],
                &[vec![
                    c.score.to_string(),
                    c.length.to_string(),
                    c.occurrences.len().to_string(),
                ]]
            )
        );
        println!();
        println!("{}", ui.section("Occurrences"));
        let rows: Vec<Vec<String>> = c
            .occurrences
            .iter()
            .map(|site| {
                vec![
                    site.container.clone(),
                    format!("{}..{}", site.start, site.end),
                ]
            })
            .collect();
        print!("{}", ui.table(&["container", "steps"], &rows));
        println!();
        println!("{}", ui.section("Sketch action.yml"));
        for line in c.sketch.lines() {
            println!("  {line}");
        }
    }
}
