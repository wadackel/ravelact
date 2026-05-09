use crate::cache;
use crate::check_permissions;
use crate::markdown;
use crate::ui::{Severity, Status, Ui};
use anyhow::Result;
use globset::GlobSet;

use crate::cli::{build_or_load, ReportFormat};
use crate::ui;

pub(in crate::cli) fn run(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    format: &ReportFormat,
    ui: &Ui,
) -> Result<i32> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    let findings = check_permissions::check(&ir);
    match format {
        ReportFormat::Markdown => {
            println!("### Permissions");
            println!();
            if findings.is_empty() {
                println!("No findings.");
            } else {
                println!(
                    "{} found.",
                    ui::plural(findings.len(), "finding", "findings")
                );
                println!();
                println!("| Severity | Kind | Location | Message |");
                println!("|---|---|---|---|");
                for f in &findings {
                    println!(
                        "| `{}` | `{}` | {} | {} |",
                        severity_label(f.severity),
                        kind_label(&f.kind),
                        markdown::code_cell(&location(&f.location, root, ui)),
                        markdown::table_cell(&f.message)
                    );
                }
            }
        }
        ReportFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&findings)?);
        }
        ReportFormat::Text => {
            if findings.is_empty() {
                println!(
                    "{}",
                    ui.status_header("permissions", Status::Clean, "no findings", &[])
                );
            } else {
                let high = findings
                    .iter()
                    .filter(|f| matches!(f.severity, check_permissions::Severity::High))
                    .count();
                let medium = findings
                    .iter()
                    .filter(|f| matches!(f.severity, check_permissions::Severity::Medium))
                    .count();
                let metadata = ui::severity_breakdown(high, medium);
                println!(
                    "{}",
                    ui.status_header(
                        "permissions",
                        Status::Error,
                        ui::plural(findings.len(), "finding", "findings"),
                        &metadata,
                    )
                );
                println!();
                for f in &findings {
                    print!(
                        "{}",
                        ui.detail_block(
                            Some(severity_for(f.severity)),
                            kind_label(&f.kind),
                            &location(&f.location, root, ui),
                            &f.message,
                        )
                    );
                }
            }
        }
    }
    Ok(if findings.is_empty() { 0 } else { 1 })
}

fn severity_label(severity: check_permissions::Severity) -> &'static str {
    match severity {
        check_permissions::Severity::High => "high",
        check_permissions::Severity::Medium => "medium",
    }
}

fn severity_for(severity: check_permissions::Severity) -> Severity {
    match severity {
        check_permissions::Severity::High => Severity::High,
        check_permissions::Severity::Medium => Severity::Medium,
    }
}

fn kind_label(kind: &check_permissions::FindingKind) -> &'static str {
    match kind {
        check_permissions::FindingKind::OverlyBroadCoarse { .. } => "overly-broad-coarse",
        check_permissions::FindingKind::CalleeEscalatesCaller { .. } => "callee-escalates-caller",
        check_permissions::FindingKind::ImplicitRepoDefault { .. } => "implicit-repo-default",
    }
}

fn location(
    location: &check_permissions::FindingLocation,
    root: &std::path::Path,
    ui: &Ui,
) -> String {
    match location {
        check_permissions::FindingLocation::Workflow { file } => ui.path(root, file),
        check_permissions::FindingLocation::Job { file, job, .. } => {
            format!("{}:{job}", ui.path(root, file))
        }
    }
}
