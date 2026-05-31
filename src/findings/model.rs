//! Core normalized finding model.
//!
//! External tools (zizmor today, actionlint / others later) emit findings in
//! their own formats. The reader layer (`sarif.rs`) normalizes them into the
//! [`Finding`] type defined here, which is the stable, tool-agnostic contract
//! the rest of the pipeline (`attach.rs`, `enrich.rs`) builds on.
//!
//! The original tool severity is preserved verbatim on [`Finding::severity`]
//! ("source severity"). Any graph-derived priority lives on a separate field
//! in [`crate::findings::enrich::EnrichedFinding`] and never overwrites this.

use std::path::PathBuf;

use serde::Serialize;

/// Deterministic identifier for a single finding, synthesized by the reader
/// from the finding's rule and location so it is stable across runs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct FindingId(pub String);

/// Which tool produced a finding.
///
/// Known tools get a typed variant; everything else is carried verbatim via
/// [`FindingSource::External`]. The reader derives this from the SARIF
/// `tool.driver.name` — it never branches command behavior on the tool name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSource {
    Ravelact,
    Actionlint,
    Zizmor,
    External(String),
}

impl FindingSource {
    /// Map a SARIF `tool.driver.name` to a source. Matching is
    /// case-insensitive; unknown names are preserved as [`FindingSource::External`].
    pub fn from_driver_name(name: &str) -> FindingSource {
        match name.to_ascii_lowercase().as_str() {
            "zizmor" => FindingSource::Zizmor,
            "actionlint" => FindingSource::Actionlint,
            "ravelact" => FindingSource::Ravelact,
            _ => FindingSource::External(name.to_string()),
        }
    }

    /// Stable lowercase label used in synthesized ids and display.
    pub fn label(&self) -> &str {
        match self {
            FindingSource::Ravelact => "ravelact",
            FindingSource::Actionlint => "actionlint",
            FindingSource::Zizmor => "zizmor",
            FindingSource::External(name) => name,
        }
    }
}

/// Normalized severity.
///
/// Variants are ordered from least to most severe so the derived `Ord` is
/// meaningful for priority comparisons (`Info < Low < Medium < High < Error`).
/// `Error` is the top tier reserved for tools that only express the coarse
/// SARIF `level = error` without a finer security severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Error,
}

/// A free-form classification tag attached by the source tool (e.g. zizmor
/// tags security audits with `"security"`). Kept as an opaque string so new
/// tags require no code change; [`FindingTag::is_security`] exposes the one
/// distinction the pipeline currently cares about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct FindingTag(pub String);

impl FindingTag {
    pub fn is_security(&self) -> bool {
        self.0.eq_ignore_ascii_case("security")
    }
}

/// Source location of a finding, as reported by the tool (a SARIF
/// `physicalLocation`). Paths are normalized to be repo-root-relative with
/// forward slashes so they line up with ravelact IR node ids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
}

/// A normalized finding: the stable contract produced by the reader and
/// consumed by attachment + enrichment. `severity` is the source severity and
/// is never mutated downstream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub id: FindingId,
    pub source: FindingSource,
    pub rule_id: String,
    pub title: String,
    pub message: String,
    pub severity: Severity,
    pub location: Location,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<FindingTag>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_source_from_driver_name_is_case_insensitive() {
        assert_eq!(
            FindingSource::from_driver_name("ZiZmOr"),
            FindingSource::Zizmor
        );
        assert_eq!(
            FindingSource::from_driver_name("ActionLint"),
            FindingSource::Actionlint
        );
        assert_eq!(
            FindingSource::from_driver_name("ravelact"),
            FindingSource::Ravelact
        );
        match FindingSource::from_driver_name("trivy") {
            FindingSource::External(name) => assert_eq!(name, "trivy"),
            other => panic!("expected External, got {other:?}"),
        }
    }

    #[test]
    fn finding_source_label_covers_every_variant() {
        assert_eq!(FindingSource::Ravelact.label(), "ravelact");
        assert_eq!(FindingSource::Actionlint.label(), "actionlint");
        assert_eq!(FindingSource::Zizmor.label(), "zizmor");
        assert_eq!(
            FindingSource::External("trivy".to_string()).label(),
            "trivy"
        );
    }
}
