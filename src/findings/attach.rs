//! Attachment resolver: pin a [`Finding`] onto an IR node and the finest
//! sub-anchor (job / step) we can justify, tagged with a [`Confidence`].
//!
//! This is the core of the overlay's trustworthiness. The IR only carries a
//! best-effort start line per workflow / job / step / action (no end line or
//! column), so resolution degrades gracefully:
//!
//! - **[`Confidence::Exact`]** — the file path matched an IR node and either the
//!   finding is file/header level (no line, or a line before any job/step), or
//!   its start line equals a job/step start line exactly.
//! - **[`Confidence::Heuristic`]** — the line fell between anchors and was tied
//!   to the nearest preceding job/step start line (e.g. a `uses:` line a couple
//!   of rows below its step's mapping start).
//! - **[`Confidence::FileOnly`]** — the path matched no IR node; unresolved.
//!
//! Sub-line precision (which `uses:` within a step) is intentionally out of
//! scope — the IR lacks the spans for it. Same-line ambiguity (multiple steps
//! starting on one line) resolves to the nearest preceding anchor and is
//! therefore Heuristic, never Exact.

use std::path::PathBuf;

use serde::Serialize;

use crate::ir::{ActionId, Ir, JobId, LocalAction, Workflow, WorkflowId};
use crate::query::impact::{classify_input, InputClassification};

use super::model::Finding;

/// The IR node a finding resolved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum NodeRef {
    Workflow {
        id: WorkflowId,
    },
    Action {
        id: ActionId,
    },
    /// The finding's path matched no workflow or action in the IR.
    Unresolved {
        path: PathBuf,
    },
}

/// Where inside a node a finding was anchored. `UsesEdge`-level findings fold
/// into their containing step (the IR has no separate edge span in M1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "anchor", rename_all = "snake_case")]
pub enum SubAnchor {
    /// The whole workflow / action file (header-level or no finer anchor).
    WorkflowFile,
    /// A specific job key (workflows only).
    Job { job: JobId },
    /// A specific step. `job` is `None` for composite-action steps.
    Step {
        #[serde(skip_serializing_if = "Option::is_none")]
        job: Option<JobId>,
        index: usize,
    },
}

/// How trustworthy the attachment is. See module docs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Exact,
    Heuristic,
    FileOnly,
}

/// The result of resolving a finding location onto the graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Attachment {
    pub node: NodeRef,
    pub sub_anchor: SubAnchor,
    pub confidence: Confidence,
    /// Human-readable note on how the attachment was derived (for auditing).
    pub reason: String,
}

/// A resolvable anchor inside a node, paired with its 1-based start line.
struct Anchor {
    line: usize,
    sub: SubAnchor,
}

/// Resolve a finding onto an IR node + sub-anchor with a confidence.
pub fn attach(ir: &Ir, finding: &Finding) -> Attachment {
    let path = finding.location.path.to_string_lossy();
    let line = finding.location.start_line.map(|l| l as usize);

    match classify_input(ir, &path) {
        InputClassification::Workflow(id) => {
            let wf = ir
                .workflows
                .iter()
                .find(|w| w.id == id)
                .expect("classify_input returned a workflow id present in the IR");
            let anchors = workflow_anchors(wf);
            let (sub_anchor, confidence, reason) = resolve(&anchors, line);
            Attachment {
                node: NodeRef::Workflow { id },
                sub_anchor,
                confidence,
                reason,
            }
        }
        InputClassification::Action(id) => {
            let act = ir
                .actions
                .iter()
                .find(|a| a.id == id)
                .expect("classify_input returned an action id present in the IR");
            let anchors = action_anchors(act);
            let (sub_anchor, confidence, reason) = resolve(&anchors, line);
            Attachment {
                node: NodeRef::Action { id },
                sub_anchor,
                confidence,
                reason,
            }
        }
        InputClassification::Unknown(_) => Attachment {
            node: NodeRef::Unresolved {
                path: finding.location.path.clone(),
            },
            sub_anchor: SubAnchor::WorkflowFile,
            confidence: Confidence::FileOnly,
            reason: "path matched no workflow or action in the IR".to_string(),
        },
    }
}

/// Build the candidate anchors (jobs + steps) for a workflow, each with its
/// 1-based start line. Anchors without a known line are skipped.
fn workflow_anchors(wf: &Workflow) -> Vec<Anchor> {
    let mut anchors = Vec::new();
    for job in &wf.jobs {
        if let Some(line) = job.source.line {
            anchors.push(Anchor {
                line,
                sub: SubAnchor::Job {
                    job: job.id.clone(),
                },
            });
        }
        for step in &job.steps {
            if let Some(line) = step.source.line {
                anchors.push(Anchor {
                    line,
                    sub: SubAnchor::Step {
                        job: Some(job.id.clone()),
                        index: step.index,
                    },
                });
            }
        }
    }
    anchors
}

/// Build the candidate anchors for a composite action's steps (no jobs).
fn action_anchors(act: &LocalAction) -> Vec<Anchor> {
    act.steps
        .iter()
        .filter_map(|step| {
            step.source.line.map(|line| Anchor {
                line,
                sub: SubAnchor::Step {
                    job: None,
                    index: step.index,
                },
            })
        })
        .collect()
}

/// Pure resolution: given a node's anchors and the finding's start line,
/// pick the sub-anchor + confidence.
///
/// - no line, or line before every anchor -> whole file, Exact (header level)
/// - line equals an anchor's start line -> that anchor, Exact (step preferred
///   over job on a tie)
/// - otherwise -> nearest preceding anchor, Heuristic
fn resolve(anchors: &[Anchor], line: Option<usize>) -> (SubAnchor, Confidence, String) {
    let Some(line) = line else {
        return (
            SubAnchor::WorkflowFile,
            Confidence::Exact,
            "file-level (finding carried no line)".to_string(),
        );
    };

    // Exact line match. Prefer the deepest anchor (step over job) when several
    // share a line.
    let exact = anchors
        .iter()
        .filter(|a| a.line == line)
        .max_by_key(|a| anchor_specificity(&a.sub));
    if let Some(a) = exact {
        return (
            a.sub.clone(),
            Confidence::Exact,
            format!("exact start-line match at line {line}"),
        );
    }

    // Nearest preceding anchor (largest start line <= finding line). On a tie
    // prefer the deepest anchor.
    let nearest = anchors
        .iter()
        .filter(|a| a.line <= line)
        .max_by_key(|a| (a.line, anchor_specificity(&a.sub)));
    match nearest {
        Some(a) => (
            a.sub.clone(),
            Confidence::Heuristic,
            format!(
                "nearest preceding anchor at line {} for finding at line {line}",
                a.line
            ),
        ),
        None => (
            SubAnchor::WorkflowFile,
            Confidence::Exact,
            format!("file/header level (line {line} precedes all jobs and steps)"),
        ),
    }
}

/// Deeper anchors win ties: Step (2) > Job (1) > WorkflowFile (0).
fn anchor_specificity(sub: &SubAnchor) -> u8 {
    match sub {
        SubAnchor::Step { .. } => 2,
        SubAnchor::Job { .. } => 1,
        SubAnchor::WorkflowFile => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job_anchor(line: usize, job: &str) -> Anchor {
        Anchor {
            line,
            sub: SubAnchor::Job {
                job: JobId(job.to_string()),
            },
        }
    }

    fn step_anchor(line: usize, job: &str, index: usize) -> Anchor {
        Anchor {
            line,
            sub: SubAnchor::Step {
                job: Some(JobId(job.to_string())),
                index,
            },
        }
    }

    // Mirrors the layout of tests/fixtures/synthetic/zizmor-findings/ci.yml:
    //   line 8  job `build`
    //   line 11 step 0 (Checkout), uses on line 12
    //   line 13 step 1 (Greet), run on line 14
    //   line 15 step 2 (Run third-party), uses on line 16
    fn ci_anchors() -> Vec<Anchor> {
        vec![
            job_anchor(8, "build"),
            step_anchor(11, "build", 0),
            step_anchor(13, "build", 1),
            step_anchor(15, "build", 2),
        ]
    }

    #[test]
    fn no_line_is_file_level_exact() {
        let (sub, conf, _) = resolve(&ci_anchors(), None);
        assert_eq!(sub, SubAnchor::WorkflowFile);
        assert_eq!(conf, Confidence::Exact);
    }

    #[test]
    fn line_before_any_anchor_is_file_level_exact() {
        // e.g. dangerous-triggers on `on:` (line 2) or permissions header.
        let (sub, conf, _) = resolve(&ci_anchors(), Some(2));
        assert_eq!(sub, SubAnchor::WorkflowFile);
        assert_eq!(conf, Confidence::Exact);
    }

    #[test]
    fn exact_step_start_line_is_exact() {
        // artipacked reported at the step's mapping start (line 11).
        let (sub, conf, _) = resolve(&ci_anchors(), Some(11));
        assert_eq!(
            sub,
            SubAnchor::Step {
                job: Some(JobId("build".to_string())),
                index: 0
            }
        );
        assert_eq!(conf, Confidence::Exact);
    }

    #[test]
    fn exact_job_start_line_is_exact() {
        let (sub, conf, _) = resolve(&ci_anchors(), Some(8));
        assert_eq!(
            sub,
            SubAnchor::Job {
                job: JobId("build".to_string())
            }
        );
        assert_eq!(conf, Confidence::Exact);
    }

    #[test]
    fn uses_line_resolves_to_nearest_preceding_step_heuristic() {
        // unpinned-uses on the `uses:` line (12) -> Checkout step (start 11).
        let (sub, conf, _) = resolve(&ci_anchors(), Some(12));
        assert_eq!(
            sub,
            SubAnchor::Step {
                job: Some(JobId("build".to_string())),
                index: 0
            }
        );
        assert_eq!(conf, Confidence::Heuristic);

        // unpinned-uses at line 16 -> third-party step (start 15).
        let (sub, conf, _) = resolve(&ci_anchors(), Some(16));
        assert_eq!(
            sub,
            SubAnchor::Step {
                job: Some(JobId("build".to_string())),
                index: 2
            }
        );
        assert_eq!(conf, Confidence::Heuristic);
    }

    #[test]
    fn line_inside_job_before_first_step_is_nearest_job_heuristic() {
        // line 9 (runs-on) sits between job start (8) and first step (11).
        let (sub, conf, _) = resolve(&ci_anchors(), Some(9));
        assert_eq!(
            sub,
            SubAnchor::Job {
                job: JobId("build".to_string())
            }
        );
        assert_eq!(conf, Confidence::Heuristic);
    }

    #[test]
    fn step_preferred_over_job_on_exact_line_tie() {
        let anchors = vec![job_anchor(10, "j"), step_anchor(10, "j", 0)];
        let (sub, conf, _) = resolve(&anchors, Some(10));
        assert_eq!(
            sub,
            SubAnchor::Step {
                job: Some(JobId("j".to_string())),
                index: 0
            }
        );
        assert_eq!(conf, Confidence::Exact);
    }

    #[test]
    fn empty_anchors_resolves_to_file_level() {
        let (sub, conf, _) = resolve(&[], Some(42));
        assert_eq!(sub, SubAnchor::WorkflowFile);
        assert_eq!(conf, Confidence::Exact);
    }
}
