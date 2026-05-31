//! End-to-end integration test for the finding-overlay normalization pipeline.
//!
//! Builds the IR from the `zizmor-findings` fixture estate, reads the committed
//! zizmor SARIF, then runs the full `read -> attach -> enrich` pipeline and
//! snapshots the resulting `EnrichedFinding`s. This exercises the IR glue
//! (path/line -> node resolution, reachability, callers/callees, orphan status,
//! permission/secret context, graph priority) that the per-module unit tests
//! cover only in isolation.
//!
//! All serialized fields are repo-root-relative (node ids, finding paths), so
//! the only normalization needed for a portable snapshot is a deterministic
//! ordering independent of reader / IR iteration order.

use std::path::Path;

use globset::GlobSet;
use ravelact::findings::attach::attach;
use ravelact::findings::enrich::enrich;
use ravelact::findings::read_findings;
use ravelact::ir::build_ir;

#[test]
fn zizmor_sarif_overlay_pipeline() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic/zizmor-findings");

    let ir = build_ir(&fixture, &GlobSet::empty()).expect("build IR from fixture");
    let findings = read_findings(&fixture.join("zizmor.sarif")).expect("read zizmor SARIF");
    assert!(!findings.is_empty(), "fixture SARIF should yield findings");

    let mut enriched: Vec<_> = findings
        .into_iter()
        .map(|finding| {
            let attachment = attach(&ir, &finding);
            enrich(&ir, finding, attachment)
        })
        .collect();

    // Stable, iteration-order-independent ordering for a portable snapshot.
    enriched.sort_by_key(|e| {
        (
            e.finding.rule_id.clone(),
            e.finding.location.path.to_string_lossy().into_owned(),
            e.finding.location.start_line,
            e.finding.location.start_column,
            e.finding.id.0.clone(),
        )
    });

    insta::assert_json_snapshot!("zizmor_overlay", enriched);
}
