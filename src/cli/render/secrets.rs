use crate::cache;
use crate::check_secrets;
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
    let findings = check_secrets::check(&ir);
    match format {
        ReportFormat::Markdown => {
            println!("### Secrets");
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
                    ui.status_header("secrets", Status::Clean, "no findings", &[])
                );
            } else {
                let high = findings
                    .iter()
                    .filter(|f| matches!(f.severity, check_secrets::Severity::High))
                    .count();
                let medium = findings
                    .iter()
                    .filter(|f| matches!(f.severity, check_secrets::Severity::Medium))
                    .count();
                let metadata = ui::severity_breakdown(high, medium);
                println!(
                    "{}",
                    ui.status_header(
                        "secrets",
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

fn severity_label(severity: check_secrets::Severity) -> &'static str {
    match severity {
        check_secrets::Severity::High => "high",
        check_secrets::Severity::Medium => "medium",
    }
}

fn severity_for(severity: check_secrets::Severity) -> Severity {
    match severity {
        check_secrets::Severity::High => Severity::High,
        check_secrets::Severity::Medium => Severity::Medium,
    }
}

fn kind_label(kind: &check_secrets::FindingKind) -> &'static str {
    match kind {
        check_secrets::FindingKind::MissingSecretPropagation { .. } => "missing-secret-propagation",
        check_secrets::FindingKind::SecretsInheritChainBreak { .. } => {
            "secrets-inherit-chain-break"
        }
        check_secrets::FindingKind::EnvironmentInWorkflowCallCallee { .. } => {
            "environment-in-workflow-call-callee"
        }
    }
}

fn location(location: &check_secrets::FindingLocation, root: &std::path::Path, ui: &Ui) -> String {
    match location {
        check_secrets::FindingLocation::Workflow { file } => ui.path(root, file),
        check_secrets::FindingLocation::Job { file, job, .. } => {
            format!("{}:{job}", ui.path(root, file))
        }
    }
}
