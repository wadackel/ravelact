//! Finding-overlay normalization layer.
//!
//! Pipeline (M1):
//!
//! ```text
//! input SARIF ──parse──▶ Finding ──attach(ir)──▶ Attachment ──enrich(ir)──▶ EnrichedFinding
//! ```
//!
//! External findings are normalized into the tool-agnostic [`model::Finding`],
//! resolved onto IR nodes/sub-anchors with a confidence by `attach`, then
//! enriched with graph context and a derived priority by `enrich`. The CLI
//! surface is intentionally untouched in M1; this module is exercised through
//! the library and integration tests.

use std::path::Path;

pub mod attach;
pub mod enrich;
pub mod model;
pub mod sarif;

pub use model::{Finding, FindingId, FindingSource, FindingTag, Location, Severity};

/// Read findings from a file. M1 supports SARIF only; the format is detected
/// from the document shape (`$schema` mentioning sarif, or a top-level `runs`
/// array). ravelact-native JSON and the actionlint adapter are out of scope.
pub fn read_findings(path: &Path) -> anyhow::Result<Vec<Finding>> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("failed to read findings file {}: {e}", path.display()))?;
    read_findings_str(&raw)
}

/// Parse findings from an in-memory string (SARIF-only in M1).
pub fn read_findings_str(raw: &str) -> anyhow::Result<Vec<Finding>> {
    if !looks_like_sarif(raw) {
        anyhow::bail!(
            "unrecognized findings format; M1 supports SARIF only \
             (expected a `$schema` mentioning sarif or a top-level `runs` array)"
        );
    }
    sarif::parse(raw)
}

/// Minimal SARIF auto-detection: a `$schema` referencing sarif, or the
/// presence of a top-level `runs` array.
fn looks_like_sarif(raw: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return false;
    };
    let schema_is_sarif = value
        .get("$schema")
        .and_then(serde_json::Value::as_str)
        .map(|s| s.to_ascii_lowercase().contains("sarif"))
        .unwrap_or(false);
    schema_is_sarif
        || value
            .get("runs")
            .map(serde_json::Value::is_array)
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_sarif_by_runs_array() {
        assert!(looks_like_sarif(r#"{"runs":[]}"#));
    }

    #[test]
    fn detects_sarif_by_schema() {
        assert!(looks_like_sarif(
            r#"{"$schema":"https://json.schemastore.org/sarif-2.1.0.json","version":"2.1.0"}"#
        ));
    }

    #[test]
    fn rejects_non_sarif_json() {
        let err = read_findings_str(r#"{"foo":"bar"}"#).unwrap_err();
        assert!(err.to_string().contains("SARIF only"));
    }

    #[test]
    fn rejects_invalid_json() {
        assert!(!looks_like_sarif("not json"));
    }
}
