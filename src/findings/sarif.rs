//! Generic SARIF v2.1.0 reader.
//!
//! Normalizes any SARIF run into [`Finding`]s without branching on the tool
//! name (a typed [`FindingSource`] is derived from `tool.driver.name`, but
//! parsing behavior is identical for every tool). SARIF is deserialized with
//! serde structs — never parsed with regexes.
//!
//! ## Severity recovery
//!
//! SARIF's per-result `level` is coarse (`error`/`warning`/`note`/`none`), so
//! finer severity is recovered in priority order:
//!
//! 1. `properties["security-severity"]` (GitHub convention, a 0–10 number)
//! 2. `result.rank` (SARIF, 0–100)
//! 3. a known per-result severity property from [`SEVERITY_PROPERTY_KEYS`]
//!    (e.g. zizmor emits `properties["zizmor/severity"] = "High"`)
//! 4. the SARIF `level`, resolving the result-level value against the rule's
//!    `defaultConfiguration.level`.

use serde::Deserialize;
use serde_json::{Map, Value};

use super::model::{Finding, FindingId, FindingSource, FindingTag, Location, Severity};

/// Per-result property keys, in preference order, that may carry a finer
/// severity than the coarse SARIF `level`. Generic across tools; extend this
/// list as new SARIF-emitting tools are adopted.
const SEVERITY_PROPERTY_KEYS: &[&str] = &["zizmor/severity", "severity"];

#[derive(Debug, Deserialize)]
struct SarifDoc {
    #[serde(default)]
    runs: Vec<SarifRun>,
}

#[derive(Debug, Deserialize)]
struct SarifRun {
    tool: SarifTool,
    #[serde(default)]
    results: Vec<SarifResult>,
}

#[derive(Debug, Deserialize)]
struct SarifTool {
    driver: SarifDriver,
}

#[derive(Debug, Deserialize)]
struct SarifDriver {
    #[serde(default)]
    name: String,
    #[serde(default)]
    rules: Vec<SarifRule>,
}

#[derive(Debug, Deserialize)]
struct SarifRule {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, rename = "shortDescription")]
    short_description: Option<SarifText>,
    #[serde(default, rename = "defaultConfiguration")]
    default_configuration: Option<SarifDefaultConfig>,
    #[serde(default)]
    properties: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct SarifDefaultConfig {
    #[serde(default)]
    level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SarifText {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SarifResult {
    #[serde(default, rename = "ruleId")]
    rule_id: Option<String>,
    #[serde(default)]
    level: Option<String>,
    #[serde(default)]
    rank: Option<f64>,
    #[serde(default)]
    message: Option<SarifText>,
    #[serde(default)]
    locations: Vec<SarifLocation>,
    #[serde(default)]
    properties: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
struct SarifLocation {
    #[serde(default, rename = "physicalLocation")]
    physical_location: Option<SarifPhysicalLocation>,
}

#[derive(Debug, Deserialize)]
struct SarifPhysicalLocation {
    #[serde(default, rename = "artifactLocation")]
    artifact_location: Option<SarifArtifactLocation>,
    #[serde(default)]
    region: Option<SarifRegion>,
}

#[derive(Debug, Deserialize)]
struct SarifArtifactLocation {
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SarifRegion {
    #[serde(default, rename = "startLine")]
    start_line: Option<u32>,
    #[serde(default, rename = "startColumn")]
    start_column: Option<u32>,
    #[serde(default, rename = "endLine")]
    end_line: Option<u32>,
    #[serde(default, rename = "endColumn")]
    end_column: Option<u32>,
}

/// Parse a SARIF document into normalized findings.
pub fn parse(json: &str) -> anyhow::Result<Vec<Finding>> {
    let doc: SarifDoc = serde_json::from_str(json)?;
    let mut findings = Vec::new();

    for run in &doc.runs {
        let source = FindingSource::from_driver_name(&run.tool.driver.name);
        let rules = RuleIndex::build(&run.tool.driver.rules);

        for result in &run.results {
            let rule_id = result.rule_id.clone().unwrap_or_default();
            let rule = rules.get(&rule_id);

            let severity = recover_severity(result, rule);
            let location = result_location(result);
            let message = result
                .message
                .as_ref()
                .and_then(|m| m.text.clone())
                .unwrap_or_default();
            let title = rule_title(&rule_id, rule);
            let tags = rule.map(collect_tags).unwrap_or_default();
            let id = synthesize_id(&source, &rule_id, &location);

            findings.push(Finding {
                id,
                source: source.clone(),
                rule_id,
                title,
                message,
                severity,
                location,
                tags,
            });
        }
    }

    Ok(findings)
}

/// Lookup of rule metadata by rule id within a single run.
struct RuleIndex<'a> {
    by_id: std::collections::HashMap<&'a str, &'a SarifRule>,
}

impl<'a> RuleIndex<'a> {
    fn build(rules: &'a [SarifRule]) -> RuleIndex<'a> {
        let by_id = rules.iter().map(|r| (r.id.as_str(), r)).collect();
        RuleIndex { by_id }
    }

    fn get(&self, rule_id: &str) -> Option<&'a SarifRule> {
        self.by_id.get(rule_id).copied()
    }
}

/// Recover a normalized severity using the documented priority order.
fn recover_severity(result: &SarifResult, rule: Option<&SarifRule>) -> Severity {
    // 1. security-severity (result props first, then rule props).
    if let Some(sev) = security_severity(&result.properties)
        .or_else(|| rule.and_then(|r| security_severity(&r.properties)))
    {
        return sev;
    }
    // 2. SARIF rank (0–100).
    if let Some(rank) = result.rank {
        return severity_from_rank(rank);
    }
    // 3. known per-result severity property (e.g. zizmor/severity).
    if let Some(sev) = severity_from_known_property(&result.properties)
        .or_else(|| rule.and_then(|r| severity_from_known_property(&r.properties)))
    {
        return sev;
    }
    // 4. SARIF level, with the rule's defaultConfiguration as fallback.
    let level = result
        .level
        .clone()
        .or_else(|| {
            rule.and_then(|r| r.default_configuration.as_ref())
                .and_then(|c| c.level.clone())
        })
        .unwrap_or_else(|| "warning".to_string());
    severity_from_level(&level)
}

/// `security-severity` is a 0–10 score conventionally stored as a string.
fn security_severity(props: &Map<String, Value>) -> Option<Severity> {
    let raw = props.get("security-severity")?;
    let score = match raw {
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    // GitHub convention: >=7 high (incl. critical >=9), 4–6.9 medium, >0 low.
    Some(if score >= 7.0 {
        Severity::High
    } else if score >= 4.0 {
        Severity::Medium
    } else if score > 0.0 {
        Severity::Low
    } else {
        Severity::Info
    })
}

fn severity_from_rank(rank: f64) -> Severity {
    if rank >= 80.0 {
        Severity::High
    } else if rank >= 50.0 {
        Severity::Medium
    } else if rank > 0.0 {
        Severity::Low
    } else {
        Severity::Info
    }
}

fn severity_from_known_property(props: &Map<String, Value>) -> Option<Severity> {
    for key in SEVERITY_PROPERTY_KEYS {
        if let Some(Value::String(s)) = props.get(*key) {
            return Some(severity_from_word(s));
        }
    }
    None
}

/// Map a severity word (from a tool property or SARIF level) to [`Severity`].
fn severity_from_word(word: &str) -> Severity {
    match word.trim().to_ascii_lowercase().as_str() {
        "critical" | "error" => Severity::Error,
        "high" => Severity::High,
        "medium" | "moderate" | "warning" => Severity::Medium,
        "low" | "note" => Severity::Low,
        "info" | "informational" | "none" => Severity::Info,
        _ => Severity::Medium,
    }
}

/// SARIF `level` is one of error/warning/note/none.
fn severity_from_level(level: &str) -> Severity {
    match level.trim().to_ascii_lowercase().as_str() {
        "error" => Severity::Error,
        "warning" => Severity::Medium,
        "note" => Severity::Low,
        "none" => Severity::Info,
        _ => Severity::Medium,
    }
}

fn rule_title(rule_id: &str, rule: Option<&SarifRule>) -> String {
    rule.and_then(|r| {
        r.short_description
            .as_ref()
            .and_then(|d| d.text.clone())
            .or_else(|| r.name.clone())
    })
    .unwrap_or_else(|| rule_id.to_string())
}

fn collect_tags(rule: &SarifRule) -> Vec<FindingTag> {
    rule.properties
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(|s| FindingTag(s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Take the primary (`locations[0]`) physical location. Paths are normalized
/// to forward slashes; absolute / `file://` prefixes are left for the
/// attachment resolver to reconcile against IR node ids.
fn result_location(result: &SarifResult) -> Location {
    let phys = result
        .locations
        .first()
        .and_then(|l| l.physical_location.as_ref());
    let uri = phys
        .and_then(|p| p.artifact_location.as_ref())
        .and_then(|a| a.uri.clone())
        .unwrap_or_default();
    let region = phys.and_then(|p| p.region.as_ref());
    Location {
        path: normalize_uri(&uri).into(),
        start_line: region.and_then(|r| r.start_line),
        start_column: region.and_then(|r| r.start_column),
        end_line: region.and_then(|r| r.end_line),
        end_column: region.and_then(|r| r.end_column),
    }
}

/// Normalize a SARIF artifact URI to a plain repo-relative forward-slash path.
fn normalize_uri(uri: &str) -> String {
    let trimmed = uri.strip_prefix("file://").unwrap_or(uri);
    trimmed.replace('\\', "/")
}

fn synthesize_id(source: &FindingSource, rule_id: &str, location: &Location) -> FindingId {
    let line = location
        .start_line
        .map(|l| l.to_string())
        .unwrap_or_else(|| "?".to_string());
    let col = location
        .start_column
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".to_string());
    FindingId(format!(
        "{}:{}:{}:{}:{}",
        source.label(),
        rule_id,
        location.path.to_string_lossy(),
        line,
        col,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(results: &str, rules: &str) -> String {
        format!(
            r#"{{
              "version": "2.1.0",
              "runs": [{{
                "tool": {{ "driver": {{ "name": "zizmor", "rules": [{rules}] }} }},
                "results": [{results}]
              }}]
            }}"#
        )
    }

    #[test]
    fn maps_driver_name_to_source() {
        let json = doc(
            r#"{ "ruleId": "x", "level": "error", "locations": [] }"#,
            "",
        );
        let findings = parse(&json).unwrap();
        assert_eq!(findings[0].source, FindingSource::Zizmor);
    }

    #[test]
    fn unknown_driver_name_is_external() {
        let json = r#"{"runs":[{"tool":{"driver":{"name":"customtool"}},"results":[
            {"ruleId":"r","level":"warning","locations":[]}]}]}"#;
        let findings = parse(json).unwrap();
        assert_eq!(
            findings[0].source,
            FindingSource::External("customtool".to_string())
        );
    }

    #[test]
    fn severity_from_level_only() {
        let json = doc(
            r#"
            { "ruleId": "e", "level": "error", "locations": [] },
            { "ruleId": "w", "level": "warning", "locations": [] },
            { "ruleId": "n", "level": "note", "locations": [] },
            { "ruleId": "z", "level": "none", "locations": [] }
        "#,
            "",
        );
        let f = parse(&json).unwrap();
        assert_eq!(f[0].severity, Severity::Error);
        assert_eq!(f[1].severity, Severity::Medium);
        assert_eq!(f[2].severity, Severity::Low);
        assert_eq!(f[3].severity, Severity::Info);
    }

    #[test]
    fn security_severity_takes_priority_over_level() {
        // level says warning, but security-severity 9.0 wins -> High.
        let json = doc(
            r#"{ "ruleId": "x", "level": "warning",
                 "properties": { "security-severity": "9.0" }, "locations": [] }"#,
            "",
        );
        let f = parse(&json).unwrap();
        assert_eq!(f[0].severity, Severity::High);
    }

    #[test]
    fn security_severity_tiers() {
        let mk = |score: &str| {
            let json = doc(
                &format!(
                    r#"{{ "ruleId": "x", "level": "note",
                          "properties": {{ "security-severity": "{score}" }}, "locations": [] }}"#
                ),
                "",
            );
            parse(&json).unwrap()[0].severity
        };
        assert_eq!(mk("9.5"), Severity::High);
        assert_eq!(mk("7.0"), Severity::High);
        assert_eq!(mk("5.0"), Severity::Medium);
        assert_eq!(mk("1.0"), Severity::Low);
        assert_eq!(mk("0"), Severity::Info);
    }

    #[test]
    fn rank_used_when_no_security_severity() {
        let json = doc(
            r#"{ "ruleId": "x", "level": "note", "rank": 85.0, "locations": [] }"#,
            "",
        );
        let f = parse(&json).unwrap();
        assert_eq!(f[0].severity, Severity::High);
    }

    #[test]
    fn known_property_recovers_finer_severity_than_level() {
        // zizmor: level=error but zizmor/severity=Medium should win over level.
        let json = doc(
            r#"{ "ruleId": "zizmor/x", "level": "error",
                 "properties": { "zizmor/severity": "Medium" }, "locations": [] }"#,
            "",
        );
        let f = parse(&json).unwrap();
        assert_eq!(f[0].severity, Severity::Medium);
    }

    #[test]
    fn priority_order_security_severity_over_known_property() {
        // Both present: security-severity (3.0 -> Low) beats zizmor/severity High.
        let json = doc(
            r#"{ "ruleId": "x", "level": "error",
                 "properties": { "security-severity": "3.0", "zizmor/severity": "High" },
                 "locations": [] }"#,
            "",
        );
        let f = parse(&json).unwrap();
        assert_eq!(f[0].severity, Severity::Low);
    }

    #[test]
    fn default_configuration_level_is_fallback() {
        // Result has no level; rule.defaultConfiguration.level = error.
        let json = doc(
            r#"{ "ruleId": "rule-1", "locations": [] }"#,
            r#"{ "id": "rule-1", "defaultConfiguration": { "level": "error" } }"#,
        );
        let f = parse(&json).unwrap();
        assert_eq!(f[0].severity, Severity::Error);
    }

    #[test]
    fn extracts_location_and_tags_and_title() {
        let json = doc(
            r#"{ "ruleId": "rule-1", "level": "error",
                 "message": { "text": "boom" },
                 "locations": [{ "physicalLocation": {
                     "artifactLocation": { "uri": ".github/workflows/ci.yml" },
                     "region": { "startLine": 12, "startColumn": 7, "endLine": 12, "endColumn": 20 }
                 }}] }"#,
            r#"{ "id": "rule-1", "shortDescription": { "text": "Rule One" },
                 "properties": { "tags": ["security", "audit"] } }"#,
        );
        let f = parse(&json).unwrap();
        assert_eq!(f[0].message, "boom");
        assert_eq!(f[0].title, "Rule One");
        assert_eq!(
            f[0].location.path.to_string_lossy(),
            ".github/workflows/ci.yml"
        );
        assert_eq!(f[0].location.start_line, Some(12));
        assert_eq!(f[0].location.start_column, Some(7));
        assert!(f[0].tags.iter().any(|t| t.is_security()));
    }

    #[test]
    fn normalizes_file_uri_prefix() {
        let json = doc(
            r#"{ "ruleId": "r", "level": "warning",
                 "locations": [{ "physicalLocation": {
                     "artifactLocation": { "uri": "file://.github/workflows/ci.yml" } } }] }"#,
            "",
        );
        let f = parse(&json).unwrap();
        assert_eq!(
            f[0].location.path.to_string_lossy(),
            ".github/workflows/ci.yml"
        );
    }

    #[test]
    fn synthesized_id_is_stable_and_distinct() {
        let json = doc(
            r#"
            { "ruleId": "r", "level": "warning", "locations": [{ "physicalLocation": {
                "artifactLocation": { "uri": "a.yml" }, "region": { "startLine": 1, "startColumn": 2 } } }] },
            { "ruleId": "r", "level": "warning", "locations": [{ "physicalLocation": {
                "artifactLocation": { "uri": "a.yml" }, "region": { "startLine": 9, "startColumn": 2 } } }] }
        "#,
            "",
        );
        let f = parse(&json).unwrap();
        assert_eq!(f[0].id, FindingId("zizmor:r:a.yml:1:2".to_string()));
        assert_ne!(f[0].id, f[1].id);
    }

    #[test]
    fn empty_runs_yields_no_findings() {
        assert!(parse(r#"{"runs":[]}"#).unwrap().is_empty());
    }
}
