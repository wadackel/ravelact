//! Ravelact comment annotation scanning + anchor resolution.
//!
//! Comments of the form `# ravelact:<verb> <ref>` express implicit dependencies
//! that the YAML schema cannot capture. Saphyr drops comments at parse time, so
//! we scan the raw source line by line, exclude lines that fall inside YAML
//! block-scalar ranges (`run: |` etc.), and bind each surviving comment to the
//! "next" Workflow / Job / Step node by line number.

use crate::ir::*;
use saphyr::{MarkedYaml, Scalar, YamlData};

/// Verb identified on a `# ravelact:<verb> <ref>` line, with the raw `<ref>`
/// token. The annotation is unresolved at this stage; resolution to a local
/// `WorkflowId` (or to `Dangling`) happens in `attach_annotations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawAnnotation {
    pub verb: AnnotationVerb,
    pub raw_target: String,
    /// 1-based line number where the comment appeared.
    pub line: usize,
}

/// Parse a single line as an ravelact comment. Returns `Some((verb, target))`
/// when the line matches the canonical form, `None` otherwise.
///
/// Canonical form: `^\s*#\s*ravelact:<verb>\s+<ref>\s*$`. Tabs are treated as
/// whitespace. `<verb>` must be a known kebab-style identifier (`dispatches`
/// or `triggers`); unknown verbs return `None` and the caller should record a
/// `ParseDiagnostic`.
///
/// This same detector is used by both the annotation scanner and the wiring
/// scanner (wiring uses it to *skip* ravelact comment lines so that
/// `# ravelact:dispatches X` is not also parsed as a `gh workflow run X` shell
/// invocation).
pub fn parse_ravelact_comment_line(line: &str) -> Option<(AnnotationVerb, &str)> {
    let trimmed = line.trim_start();
    let after_hash = trimmed.strip_prefix('#')?.trim_start();
    let body = after_hash.strip_prefix("ravelact:")?;
    let (verb_str, target) = body.split_once(|c: char| c.is_whitespace())?;
    let verb = match verb_str {
        "dispatches" => AnnotationVerb::Dispatches,
        "triggers" => AnnotationVerb::Triggers,
        _ => return None,
    };
    let trimmed_target = target.trim();
    if trimmed_target.is_empty() {
        return None;
    }
    Some((verb, trimmed_target))
}

/// Returns true when the comment-scan detector recognizes the line as an
/// ravelact comment, regardless of whether the verb is one we currently
/// understand. Used by the wiring scanner to skip annotation lines without
/// duplicating the detector logic.
pub fn line_starts_with_ravelact(line: &str) -> bool {
    let trimmed = line.trim_start();
    let Some(after_hash) = trimmed.strip_prefix('#') else {
        return false;
    };
    after_hash.trim_start().starts_with("ravelact:")
}

/// Half-open line range `[start, end)` of a multi-line YAML block scalar, used
/// to exclude ravelact comments that appear inside `run: |` bodies (where they
/// are part of the shell command, not YAML structure).
type ScalarRange = (usize, usize);

/// Walk a saphyr-parsed document and collect line ranges of every multi-line
/// string scalar. Single-line scalars are skipped — they cannot contain a
/// `# ...` comment line that would be confused for an ravelact annotation.
pub(crate) fn collect_block_scalar_ranges(node: &MarkedYaml<'_>, out: &mut Vec<ScalarRange>) {
    match &node.data {
        YamlData::Value(Scalar::String(_)) => {
            let start = node.span.start.line();
            let end = node.span.end.line();
            if end > start {
                out.push((start, end));
            }
        }
        YamlData::Mapping(map) => {
            for (k, v) in map.iter() {
                collect_block_scalar_ranges(k, out);
                collect_block_scalar_ranges(v, out);
            }
        }
        YamlData::Sequence(seq) => {
            for item in seq {
                collect_block_scalar_ranges(item, out);
            }
        }
        _ => {}
    }
}

/// Scan raw source for ravelact comments, dropping any whose line falls inside a
/// block-scalar range (because those `#` characters are part of a shell
/// command, not a YAML comment).
///
/// Unknown ravelact verbs and lines that look like attempts but fail to parse
/// emit a `ParseDiagnostic` and are dropped.
pub fn scan_ravelact_comments(
    raw: &str,
    file: &std::path::Path,
    scalar_ranges: &[ScalarRange],
    diags: &mut Vec<ParseDiagnostic>,
) -> Vec<RawAnnotation> {
    let mut out = Vec::new();
    for (idx, line) in raw.lines().enumerate() {
        let line_no = idx + 1; // 1-based
        if scalar_ranges
            .iter()
            .any(|(s, e)| line_no >= *s && line_no < *e)
        {
            continue;
        }
        if !line_starts_with_ravelact(line) {
            continue;
        }
        if let Some((verb, target)) = parse_ravelact_comment_line(line) {
            out.push(RawAnnotation {
                verb,
                raw_target: target.to_string(),
                line: line_no,
            });
        } else {
            // Looks like an ravelact comment but the verb / target failed to
            // parse. Surface a diagnostic so the user notices the typo.
            diags.push(ParseDiagnostic {
                file: file.to_path_buf(),
                line: line_no,
                message: format!("unrecognised ravelact comment: {}", line.trim()),
            });
        }
    }
    out
}

/// Internal anchor reference used by `attach_annotations`. Not exposed.
#[derive(Debug, Clone, Copy)]
enum NodeRef {
    Workflow,
    Job(usize),
    Step { job: usize, step: usize },
}

/// Resolve a raw `<ref>` to either a `Resolved { target }` or
/// `Dangling { raw_target, reason }`.
///
/// Accepted form (per plan): `.github/workflows/<name>.{yml,yaml}` with an
/// optional leading `./`. Anything else is dangling — including paths
/// containing `..`, absolute paths, and refs that don't match the workflow
/// directory layout.
fn resolve_target(raw: &str) -> AnnotationResolution {
    let normalized = raw.trim_start_matches("./").trim_end_matches('/');

    if normalized.is_empty() {
        return AnnotationResolution::Dangling {
            raw_target: raw.to_string(),
            reason: "empty target".into(),
        };
    }
    if normalized.starts_with('/') {
        return AnnotationResolution::Dangling {
            raw_target: raw.to_string(),
            reason: "absolute path is not allowed".into(),
        };
    }
    if normalized
        .split('/')
        .any(|seg| seg == ".." || seg == "." || seg.is_empty())
    {
        return AnnotationResolution::Dangling {
            raw_target: raw.to_string(),
            reason: "path must not contain `..`, `.`, or empty segments".into(),
        };
    }
    if !(normalized.ends_with(".yml") || normalized.ends_with(".yaml")) {
        return AnnotationResolution::Dangling {
            raw_target: raw.to_string(),
            reason: "target must be a .yml/.yaml workflow path".into(),
        };
    }
    if !normalized.starts_with(".github/workflows/") {
        return AnnotationResolution::Dangling {
            raw_target: raw.to_string(),
            reason: "target must live under .github/workflows/".into(),
        };
    }
    AnnotationResolution::Resolved {
        target: WorkflowId(normalized.to_string()),
    }
}

/// Bind raw annotations to their anchor nodes in the workflow IR.
///
/// Anchor: each annotation attaches to the Workflow / Job / Step node whose
/// `source.line` is the smallest value strictly greater than the comment's own
/// line. If no such node exists (annotation at end of file), the annotation
/// attaches to the Workflow root and a diagnostic is emitted.
pub fn attach_annotations(
    wf: &mut Workflow,
    raws: Vec<RawAnnotation>,
    diags: &mut Vec<ParseDiagnostic>,
) {
    if raws.is_empty() {
        return;
    }
    let nodes = collect_nodes_with_lines(wf);
    let file = wf.source.file.clone();

    for raw in raws {
        let anchor = nodes
            .iter()
            .filter(|(line, _)| *line > raw.line)
            .min_by_key(|(line, _)| *line)
            .map(|(_, n)| *n)
            .unwrap_or_else(|| {
                diags.push(ParseDiagnostic {
                    file: file.clone(),
                    line: raw.line,
                    message: "trailing ravelact comment, attaching to workflow root".into(),
                });
                NodeRef::Workflow
            });

        let resolution = resolve_target(&raw.raw_target);
        if let AnnotationResolution::Dangling { ref reason, .. } = resolution {
            diags.push(ParseDiagnostic {
                file: file.clone(),
                line: raw.line,
                message: format!(
                    "ravelact:{} target `{}` is dangling: {}",
                    verb_display(raw.verb),
                    raw.raw_target,
                    reason
                ),
            });
        }

        let ann = Annotation {
            verb: raw.verb,
            resolution,
            source_line: raw.line,
        };
        push_annotation(wf, anchor, ann);
    }
}

fn verb_display(v: AnnotationVerb) -> &'static str {
    match v {
        AnnotationVerb::Dispatches => "dispatches",
        AnnotationVerb::Triggers => "triggers",
    }
}

fn collect_nodes_with_lines(wf: &Workflow) -> Vec<(usize, NodeRef)> {
    let mut out: Vec<(usize, NodeRef)> = Vec::new();
    if let Some(line) = wf.source.line {
        out.push((line, NodeRef::Workflow));
    }
    for (j, job) in wf.jobs.iter().enumerate() {
        if let Some(line) = job.source.line {
            out.push((line, NodeRef::Job(j)));
        }
        for (s, step) in job.steps.iter().enumerate() {
            if let Some(line) = step.source.line {
                out.push((line, NodeRef::Step { job: j, step: s }));
            }
        }
    }
    out.sort_by_key(|(line, _)| *line);
    out
}

/// Bind raw annotations to their anchor nodes in a local action IR.
///
/// Anchor semantics mirror `attach_annotations` for workflows: each annotation
/// attaches to the `LocalAction` root or one of its `Step` nodes — whichever has
/// the smallest `source.line` strictly greater than the comment's own line.
/// Trailing annotations (past all nodes) attach to the action root.
pub fn attach_local_action_annotations(
    action: &mut LocalAction,
    raws: Vec<RawAnnotation>,
    diags: &mut Vec<ParseDiagnostic>,
) {
    if raws.is_empty() {
        return;
    }

    // Collect (line, step_index | action_root) pairs.
    let mut nodes: Vec<(usize, Option<usize>)> = Vec::new();
    if let Some(line) = action.source.line {
        nodes.push((line, None)); // None = action root
    }
    for (s, step) in action.steps.iter().enumerate() {
        if let Some(line) = step.source.line {
            nodes.push((line, Some(s)));
        }
    }
    nodes.sort_by_key(|(line, _)| *line);

    let file = action.source.file.clone();

    for raw in raws {
        let anchor: Option<usize> = nodes
            .iter()
            .filter(|(line, _)| *line > raw.line)
            .min_by_key(|(line, _)| *line)
            .map(|(_, idx)| *idx)
            .unwrap_or_else(|| {
                diags.push(ParseDiagnostic {
                    file: file.clone(),
                    line: raw.line,
                    message: "trailing ravelact comment, attaching to local action root".into(),
                });
                None // action root
            });

        let resolution = resolve_target(&raw.raw_target);
        if let AnnotationResolution::Dangling { ref reason, .. } = resolution {
            diags.push(ParseDiagnostic {
                file: file.clone(),
                line: raw.line,
                message: format!(
                    "ravelact:{} target `{}` is dangling: {}",
                    verb_display(raw.verb),
                    raw.raw_target,
                    reason
                ),
            });
        }

        let ann = Annotation {
            verb: raw.verb,
            resolution,
            source_line: raw.line,
        };

        match anchor {
            None => action.annotations.push(ann),
            Some(step_idx) => {
                if let Some(step) = action.steps.get_mut(step_idx) {
                    step.annotations.push(ann);
                } else {
                    action.annotations.push(ann);
                }
            }
        }
    }
}

fn push_annotation(wf: &mut Workflow, anchor: NodeRef, ann: Annotation) {
    match anchor {
        NodeRef::Workflow => wf.annotations.push(ann),
        NodeRef::Job(j) => {
            if let Some(job) = wf.jobs.get_mut(j) {
                job.annotations.push(ann);
            } else {
                wf.annotations.push(ann);
            }
        }
        NodeRef::Step { job, step } => {
            if let Some(s) = wf.jobs.get_mut(job).and_then(|j| j.steps.get_mut(step)) {
                s.annotations.push(ann);
            } else {
                wf.annotations.push(ann);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diag(line: usize, msg: &str) -> ParseDiagnostic {
        ParseDiagnostic {
            file: std::path::PathBuf::from("test.yml"),
            line,
            message: msg.into(),
        }
    }

    // -- parse_ravelact_comment_line ----------------------------------------------

    #[test]
    fn comment_line_basic() {
        let (verb, target) =
            parse_ravelact_comment_line("# ravelact:dispatches .github/workflows/build.yml")
                .unwrap();
        assert_eq!(verb, AnnotationVerb::Dispatches);
        assert_eq!(target, ".github/workflows/build.yml");
    }

    #[test]
    fn comment_line_indented_and_tabs() {
        let (verb, target) =
            parse_ravelact_comment_line("    \t#\travelact:triggers\t.github/workflows/x.yml")
                .unwrap();
        assert_eq!(verb, AnnotationVerb::Triggers);
        assert_eq!(target, ".github/workflows/x.yml");
    }

    #[test]
    fn comment_line_unknown_verb() {
        assert!(parse_ravelact_comment_line("# ravelact:foo .github/workflows/x.yml").is_none());
    }

    #[test]
    fn comment_line_missing_colon() {
        assert!(parse_ravelact_comment_line("# ravelact dispatches X").is_none());
    }

    #[test]
    fn comment_line_no_target() {
        assert!(parse_ravelact_comment_line("# ravelact:dispatches").is_none());
        assert!(parse_ravelact_comment_line("# ravelact:dispatches   ").is_none());
    }

    #[test]
    fn comment_line_extra_tokens_keeps_first_target() {
        // We split on the first whitespace after the verb, so anything that
        // follows is treated as part of the target token after trimming.
        // The plan deliberately scopes <ref> to a single \\S+ token; any
        // trailing junk becomes part of raw_target and will fail `resolve_target`.
        let (verb, target) =
            parse_ravelact_comment_line("# ravelact:dispatches build.yml extra-junk").unwrap();
        assert_eq!(verb, AnnotationVerb::Dispatches);
        // The whitespace-trimmed target may include the trailing tokens; the
        // resolver below catches malformed targets.
        assert!(target.starts_with("build.yml"));
    }

    // -- line_starts_with_ravelact ------------------------------------------------

    #[test]
    fn line_starts_with_ravelact_basic() {
        assert!(line_starts_with_ravelact("# ravelact:dispatches X"));
        assert!(line_starts_with_ravelact("    # ravelact:foo"));
        assert!(!line_starts_with_ravelact("echo hi"));
        assert!(!line_starts_with_ravelact("# normal comment"));
        assert!(!line_starts_with_ravelact("// ravelact:something"));
    }

    // -- scan_ravelact_comments ---------------------------------------------------

    #[test]
    fn scan_basic_and_consecutive() {
        let raw = "name: CI\n# ravelact:dispatches .github/workflows/build.yml\n# ravelact:triggers .github/workflows/notify.yml\non: push\n";
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        let scans = scan_ravelact_comments(raw, std::path::Path::new("ci.yml"), &[], &mut diags);
        assert!(diags.is_empty());
        assert_eq!(scans.len(), 2);
        assert_eq!(scans[0].verb, AnnotationVerb::Dispatches);
        assert_eq!(scans[0].line, 2);
        assert_eq!(scans[1].verb, AnnotationVerb::Triggers);
        assert_eq!(scans[1].line, 3);
    }

    #[test]
    fn scan_ignores_block_scalar_range() {
        let raw = "jobs:\n  t:\n    steps:\n      - run: |\n          # ravelact:dispatches build.yml\n          echo hi\n      - run: echo done\n";
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        // saphyr would report the `run: |` value scalar as occupying lines 5..7
        // (start = first content line, end = first non-content line).
        let scans =
            scan_ravelact_comments(raw, std::path::Path::new("ci.yml"), &[(5, 7)], &mut diags);
        assert!(diags.is_empty());
        assert!(scans.is_empty(), "block-scalar comments must be ignored");
    }

    #[test]
    fn scan_unknown_verb_emits_diagnostic() {
        let raw = "# ravelact:explode build.yml\n";
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        let scans = scan_ravelact_comments(raw, std::path::Path::new("x.yml"), &[], &mut diags);
        assert!(scans.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, 1);
        assert!(diags[0].message.contains("unrecognised ravelact comment"));
    }

    // -- attach_annotations -----------------------------------------------------

    /// Build a fixture workflow: doc starts at line 1, single job at line 5
    /// with two steps at lines 7 and 9. Mirrors a real `.yml` shape so anchor
    /// resolution exercises Step/Job/Workflow tiers without falling back.
    fn fixture_workflow() -> Workflow {
        let wf_file = std::path::PathBuf::from(".github/workflows/ci.yml");
        Workflow {
            id: WorkflowId(".github/workflows/ci.yml".into()),
            source: SourcePos {
                file: wf_file.clone(),
                line: Some(1),
            },
            name: None,
            run_name: None,
            triggers: vec![],
            jobs: vec![Job {
                id: JobId("test".into()),
                workflow: WorkflowId(".github/workflows/ci.yml".into()),
                needs: vec![],
                permissions: None,
                steps: vec![
                    Step {
                        index: 0,
                        id: None,
                        name: None,
                        uses: None,
                        run: Some("echo hi".into()),
                        if_expr: None,
                        with: Default::default(),
                        env: Default::default(),
                        shell: None,
                        working_directory: None,
                        timeout_minutes: None,
                        continue_on_error: None,
                        source: SourcePos {
                            file: wf_file.clone(),
                            line: Some(7),
                        },
                        annotations: Vec::new(),
                    },
                    Step {
                        index: 1,
                        id: None,
                        name: None,
                        uses: None,
                        run: Some("echo bye".into()),
                        if_expr: None,
                        with: Default::default(),
                        env: Default::default(),
                        shell: None,
                        working_directory: None,
                        timeout_minutes: None,
                        continue_on_error: None,
                        source: SourcePos {
                            file: wf_file.clone(),
                            line: Some(9),
                        },
                        annotations: Vec::new(),
                    },
                ],
                calls_workflow: None,
                runs_on: None,
                outputs: Default::default(),
                source: SourcePos {
                    file: wf_file,
                    line: Some(5),
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

    #[test]
    fn attach_anchors_step_for_comment_immediately_before_step() {
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        // Comment on line 8 — between step 0 (line 7) and step 1 (line 9) →
        // anchors to step 1 (smallest line > 8).
        attach_annotations(
            &mut wf,
            vec![RawAnnotation {
                verb: AnnotationVerb::Dispatches,
                raw_target: ".github/workflows/build.yml".into(),
                line: 8,
            }],
            &mut diags,
        );
        assert!(diags.is_empty(), "no diagnostics expected, got {diags:?}");
        assert!(wf.annotations.is_empty());
        assert!(wf.jobs[0].annotations.is_empty());
        assert_eq!(wf.jobs[0].steps[1].annotations.len(), 1);
        match &wf.jobs[0].steps[1].annotations[0].resolution {
            AnnotationResolution::Resolved { target } => {
                assert_eq!(target.0, ".github/workflows/build.yml");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn attach_anchors_job_for_comment_above_job() {
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        // Comment on line 4 — above the job key (line 5), no earlier step →
        // anchors to job (smallest line > 4 = 5).
        attach_annotations(
            &mut wf,
            vec![RawAnnotation {
                verb: AnnotationVerb::Triggers,
                raw_target: ".github/workflows/notify.yml".into(),
                line: 4,
            }],
            &mut diags,
        );
        assert!(diags.is_empty());
        assert_eq!(wf.jobs[0].annotations.len(), 1);
        assert!(wf.annotations.is_empty());
    }

    #[test]
    fn attach_dangling_for_dotdot() {
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        attach_annotations(
            &mut wf,
            vec![RawAnnotation {
                verb: AnnotationVerb::Dispatches,
                raw_target: "../bad/path.yml".into(),
                line: 4,
            }],
            &mut diags,
        );
        assert_eq!(diags.len(), 1, "dangling produces a diagnostic");
        assert_eq!(wf.jobs[0].annotations.len(), 1, "annotation is still kept");
        assert!(matches!(
            wf.jobs[0].annotations[0].resolution,
            AnnotationResolution::Dangling { .. }
        ));
    }

    #[test]
    fn attach_dangling_for_external_or_non_workflows_path() {
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        attach_annotations(
            &mut wf,
            vec![
                RawAnnotation {
                    verb: AnnotationVerb::Dispatches,
                    raw_target: "octo/repo/.github/workflows/x.yml@main".into(),
                    line: 2,
                },
                RawAnnotation {
                    verb: AnnotationVerb::Dispatches,
                    raw_target: "scripts/foo.sh".into(),
                    line: 4,
                },
            ],
            &mut diags,
        );
        // Both anchor below: line 2 → workflow (line 1) is < 2 so look for
        // smallest > 2, which is the job at line 5. Both attach to the job.
        assert_eq!(wf.jobs[0].annotations.len(), 2);
        for ann in &wf.jobs[0].annotations {
            assert!(
                matches!(ann.resolution, AnnotationResolution::Dangling { .. }),
                "expected Dangling for non-workflow paths"
            );
        }
        assert_eq!(diags.len(), 2);
    }

    #[test]
    fn attach_trailing_comment_falls_back_to_workflow_with_diagnostic() {
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        // Comment on line 50 — past every node → trailing fallback.
        attach_annotations(
            &mut wf,
            vec![RawAnnotation {
                verb: AnnotationVerb::Dispatches,
                raw_target: ".github/workflows/build.yml".into(),
                line: 50,
            }],
            &mut diags,
        );
        assert_eq!(wf.annotations.len(), 1);
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("trailing ravelact")),
            "expected trailing-fallback diagnostic, got: {diags:?}"
        );
    }

    #[test]
    fn attach_consecutive_comments_share_anchor() {
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        // Both comments above step 0 (line 7), no step between them →
        // both attach to step 0.
        attach_annotations(
            &mut wf,
            vec![
                RawAnnotation {
                    verb: AnnotationVerb::Dispatches,
                    raw_target: ".github/workflows/a.yml".into(),
                    line: 6,
                },
                RawAnnotation {
                    verb: AnnotationVerb::Triggers,
                    raw_target: ".github/workflows/b.yml".into(),
                    line: 6,
                },
            ],
            &mut diags,
        );
        assert!(diags.is_empty());
        assert_eq!(wf.jobs[0].steps[0].annotations.len(), 2);
        assert_eq!(wf.jobs[0].steps[0].annotations[0].source_line, 6);
        assert_eq!(wf.jobs[0].steps[0].annotations[1].source_line, 6);
    }

    // Sanity: diag helper is consumed somewhere
    #[test]
    fn _diag_helper_is_used() {
        let d = diag(7, "x");
        assert_eq!(d.line, 7);
    }

    // -- additional verb x anchor matrix coverage ------------------------------
    //
    // Anchor tiers in the parser are `_workflow`, `_job`, `_step <index>`. The
    // existing tests above cover several combos; the block below fills in the
    // remaining triggers x {step, workflow} cells and asserts source_line
    // tracking explicitly.

    #[test]
    fn attach_anchors_step_for_triggers_verb() {
        // triggers x _step combo. Comment on line 8 anchors to step 1 (line 9).
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        attach_annotations(
            &mut wf,
            vec![RawAnnotation {
                verb: AnnotationVerb::Triggers,
                raw_target: ".github/workflows/notify.yml".into(),
                line: 8,
            }],
            &mut diags,
        );
        assert!(diags.is_empty());
        assert_eq!(wf.jobs[0].steps[1].annotations.len(), 1);
        assert_eq!(
            wf.jobs[0].steps[1].annotations[0].verb,
            AnnotationVerb::Triggers
        );
        // Line-number tracking: the IR carries the comment's own line, not the
        // anchor node's line.
        assert_eq!(
            wf.jobs[0].steps[1].annotations[0].source_line, 8,
            "source_line should reflect the comment line, not the anchor"
        );
    }

    #[test]
    fn attach_anchors_workflow_for_triggers_trailing() {
        // triggers x _workflow (trailing fallback). Comment past every node.
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        attach_annotations(
            &mut wf,
            vec![RawAnnotation {
                verb: AnnotationVerb::Triggers,
                raw_target: ".github/workflows/build.yml".into(),
                line: 99,
            }],
            &mut diags,
        );
        assert_eq!(wf.annotations.len(), 1);
        assert_eq!(wf.annotations[0].verb, AnnotationVerb::Triggers);
        // Trailing-fallback diagnostic was emitted.
        assert!(diags
            .iter()
            .any(|d| d.message.contains("trailing ravelact")));
        // Diagnostic line equals the comment line (line tracking, take 2).
        let trailing = diags
            .iter()
            .find(|d| d.message.contains("trailing ravelact"))
            .unwrap();
        assert_eq!(trailing.line, 99);
    }

    /// Negative: missing target (no `<ref>` after the verb) — entire line is
    /// rejected by `parse_ravelact_comment_line`, so `scan_ravelact_comments`
    /// surfaces an "unrecognised ravelact comment" diagnostic.
    #[test]
    fn scan_missing_target_emits_diagnostic() {
        let raw = "# ravelact:dispatches\n";
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        let scans = scan_ravelact_comments(raw, std::path::Path::new("x.yml"), &[], &mut diags);
        assert!(scans.is_empty());
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("unrecognised ravelact comment"));
    }

    /// Multi-line: several ravelact comments on consecutive and separated lines
    /// must each get a distinct `RawAnnotation` with its own `line`.
    #[test]
    fn scan_multiline_distinct_line_numbers() {
        let raw = "name: CI\n\
                   # ravelact:dispatches .github/workflows/a.yml\n\
                   on: push\n\
                   # ravelact:triggers .github/workflows/b.yml\n\
                   jobs:\n\
                   # ravelact:dispatches .github/workflows/c.yml\n";
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        let scans = scan_ravelact_comments(raw, std::path::Path::new("ci.yml"), &[], &mut diags);
        assert!(diags.is_empty());
        assert_eq!(scans.len(), 3);
        assert_eq!(scans[0].line, 2);
        assert_eq!(scans[1].line, 4);
        assert_eq!(scans[2].line, 6);
        assert_eq!(scans[0].verb, AnnotationVerb::Dispatches);
        assert_eq!(scans[1].verb, AnnotationVerb::Triggers);
    }

    // -- attach_local_action_annotations ---------------------------------------
    //
    // Mirror of `attach_annotations` for composite-action manifests. Anchors
    // are: action root or step <index>. The tests below exercise both tiers,
    // both verbs, and the trailing fallback.

    fn fixture_local_action() -> LocalAction {
        let file = std::path::PathBuf::from(".github/actions/x/action.yml");
        LocalAction {
            id: ActionId(".github/actions/x".into()),
            source: SourcePos {
                file: file.clone(),
                line: Some(1),
            },
            name: Some("X".into()),
            kind: ActionKind::Composite,
            inputs: Vec::new(),
            outputs: Vec::new(),
            steps: vec![
                Step {
                    index: 0,
                    id: None,
                    name: None,
                    uses: None,
                    run: Some("echo hi".into()),
                    if_expr: None,
                    with: Default::default(),
                    env: Default::default(),
                    shell: None,
                    working_directory: None,
                    timeout_minutes: None,
                    continue_on_error: None,
                    source: SourcePos {
                        file: file.clone(),
                        line: Some(7),
                    },
                    annotations: Vec::new(),
                },
                Step {
                    index: 1,
                    id: None,
                    name: None,
                    uses: None,
                    run: Some("echo bye".into()),
                    if_expr: None,
                    with: Default::default(),
                    env: Default::default(),
                    shell: None,
                    working_directory: None,
                    timeout_minutes: None,
                    continue_on_error: None,
                    source: SourcePos {
                        file,
                        line: Some(11),
                    },
                    annotations: Vec::new(),
                },
            ],
            annotations: Vec::new(),
        }
    }

    #[test]
    fn attach_local_action_step_for_dispatches() {
        let mut action = fixture_local_action();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        // Comment on line 9 — between step 0 (line 7) and step 1 (line 11) →
        // anchors to step 1.
        attach_local_action_annotations(
            &mut action,
            vec![RawAnnotation {
                verb: AnnotationVerb::Dispatches,
                raw_target: ".github/workflows/build.yml".into(),
                line: 9,
            }],
            &mut diags,
        );
        assert!(diags.is_empty());
        assert!(action.annotations.is_empty());
        assert_eq!(action.steps[1].annotations.len(), 1);
        assert!(matches!(
            action.steps[1].annotations[0].resolution,
            AnnotationResolution::Resolved { .. }
        ));
    }

    #[test]
    fn attach_local_action_root_for_triggers_trailing() {
        let mut action = fixture_local_action();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        // Comment on line 99 — past every step → fallback to action root, with
        // a trailing diagnostic emitted.
        attach_local_action_annotations(
            &mut action,
            vec![RawAnnotation {
                verb: AnnotationVerb::Triggers,
                raw_target: ".github/workflows/notify.yml".into(),
                line: 99,
            }],
            &mut diags,
        );
        assert_eq!(action.annotations.len(), 1);
        assert_eq!(action.annotations[0].verb, AnnotationVerb::Triggers);
        assert!(diags.iter().any(|d| d
            .message
            .contains("trailing ravelact comment, attaching to local action root")));
    }

    /// Negative: dangling target inside a local action emits a diagnostic but
    /// preserves the annotation on its anchor node.
    #[test]
    fn attach_local_action_dangling_keeps_annotation() {
        let mut action = fixture_local_action();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        attach_local_action_annotations(
            &mut action,
            vec![RawAnnotation {
                verb: AnnotationVerb::Dispatches,
                raw_target: "scripts/foo.sh".into(),
                line: 6,
            }],
            &mut diags,
        );
        assert_eq!(diags.len(), 1, "dangling produces a diagnostic");
        assert!(diags[0].message.contains("dangling"));
        // Comment on line 6 anchors to step 0 (line 7).
        assert_eq!(action.steps[0].annotations.len(), 1);
        assert!(matches!(
            action.steps[0].annotations[0].resolution,
            AnnotationResolution::Dangling { .. }
        ));
    }

    /// Empty `raws` is a fast no-op path on both attach helpers; exercising
    /// it guarantees no diagnostics are emitted spuriously.
    #[test]
    fn attach_helpers_noop_on_empty_input() {
        let mut wf = fixture_workflow();
        let mut diags: Vec<ParseDiagnostic> = Vec::new();
        attach_annotations(&mut wf, vec![], &mut diags);
        assert!(diags.is_empty());
        assert!(wf.annotations.is_empty());

        let mut action = fixture_local_action();
        attach_local_action_annotations(&mut action, vec![], &mut diags);
        assert!(diags.is_empty());
        assert!(action.annotations.is_empty());
    }
}
