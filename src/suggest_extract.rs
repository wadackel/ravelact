//! `suggest extract` — detect duplicated step sequences across workflows /
//! composite actions and propose composite-action extraction candidates.

use crate::ir::{ActionId, Ir, Step, UsesRef};
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Site {
    pub container: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Candidate {
    pub length: usize,
    pub occurrences: Vec<Site>,
    pub score: usize,
    pub sketch: String,
}

/// Reduce a step to a normalized signature for duplicate-sequence matching.
/// `uses:` wins over `run:` when both are present (real workflows do not have
/// both). `with:`, `env:`, `if:`, `name:` are intentionally ignored. Shell
/// preambles (`set -e[uo pipefail]?`) and shebangs are skipped before picking
/// the first non-empty line of `run:`.
///
/// Returning `None` makes the step act as a "wall" in window enumeration
/// (no candidate window crosses it). `UsesRef::LocalWorkflow` returns `None`
/// because reusable workflows can only be invoked at the job level — they
/// cannot legally appear inside the `runs.steps` of a composite action, so
/// any extracted sketch containing one would be invalid.
pub fn normalize_step(step: &Step) -> Option<String> {
    if let Some(uses) = &step.uses {
        let canonical = match uses {
            // Walled off: a reusable workflow call cannot live inside a
            // composite action. Treat it like an unmatched step so windows
            // do not extend through it.
            UsesRef::LocalWorkflow(_) => return None,
            UsesRef::LocalAction(ActionId(p)) => format!("local-action:{p}"),
            UsesRef::External {
                owner,
                repo,
                subpath,
                gitref,
            } => match subpath {
                Some(sub) => format!("external:{owner}/{repo}/{sub}@{gitref}"),
                None => format!("external:{owner}/{repo}@{gitref}"),
            },
            UsesRef::Docker(d) => format!("docker:{}", d.display_str()),
        };
        return Some(format!("uses:{canonical}"));
    }
    if let Some(run) = &step.run {
        return first_meaningful_line(run).map(|l| format!("run:{l}"));
    }
    None
}

fn first_meaningful_line(body: &str) -> Option<String> {
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("#!") {
            continue;
        }
        if is_shell_preamble(line) {
            continue;
        }
        return Some(line.to_string());
    }
    None
}

/// Match the spec regex `^set\s+-[eu]+(o\s+pipefail)?$` on a single trimmed line.
fn is_shell_preamble(line: &str) -> bool {
    let mut parts = line.split_whitespace();
    if parts.next() != Some("set") {
        return false;
    }
    let flags = match parts.next() {
        Some(f) => f,
        None => return false,
    };
    if !flags.starts_with('-') || flags.len() < 2 {
        return false;
    }
    let chars = &flags[1..];
    let last_is_o = chars.ends_with('o');
    let prefix = if last_is_o {
        &chars[..chars.len() - 1]
    } else {
        chars
    };
    if prefix.is_empty() || !prefix.chars().all(|c| c == 'e' || c == 'u') {
        return false;
    }
    if last_is_o {
        parts.next() == Some("pipefail") && parts.next().is_none()
    } else {
        parts.next().is_none()
    }
}

#[derive(Debug, Clone)]
struct Occ {
    container: usize,
    start: usize,
    length: usize,
}

struct Container<'a> {
    id: String,
    normalized: Vec<Option<String>>,
    raw: Vec<&'a Step>,
}

pub fn find_candidates(ir: &Ir, min_length: usize, min_occurrences: usize) -> Vec<Candidate> {
    if min_length == 0 || min_occurrences < 2 {
        return Vec::new();
    }

    // 1. Container enumeration.
    let mut containers: Vec<Container<'_>> = Vec::new();
    for wf in &ir.workflows {
        for job in &wf.jobs {
            let id = format!("{}:{}", wf.id.0, job.id.0);
            let raw: Vec<&Step> = job.steps.iter().collect();
            let normalized: Vec<Option<String>> = raw.iter().map(|s| normalize_step(s)).collect();
            containers.push(Container {
                id,
                normalized,
                raw,
            });
        }
    }
    for comp in &ir.actions {
        let id = format!("{}:_composite", comp.id.0);
        let raw: Vec<&Step> = comp.steps.iter().collect();
        let normalized: Vec<Option<String>> = raw.iter().map(|s| normalize_step(s)).collect();
        containers.push(Container {
            id,
            normalized,
            raw,
        });
    }

    // 2. Window enumeration. A `None` signature is a wall — windows do not
    // extend through it, so unmatched steps split a container into segments.
    let mut buckets: BTreeMap<Vec<String>, Vec<Occ>> = BTreeMap::new();
    for (ci, container) in containers.iter().enumerate() {
        let n = container.normalized.len();
        for start in 0..n {
            let mut window: Vec<String> = Vec::new();
            for sig in container.normalized.iter().skip(start) {
                match sig {
                    Some(s) => window.push(s.clone()),
                    None => break,
                }
                if window.len() >= min_length {
                    buckets.entry(window.clone()).or_default().push(Occ {
                        container: ci,
                        start,
                        length: window.len(),
                    });
                }
            }
        }
    }

    // 3. Threshold filter.
    let mut candidates_raw: Vec<(Vec<String>, Vec<Occ>)> = buckets
        .into_iter()
        .filter(|(_, occs)| occs.len() >= min_occurrences)
        .collect();

    // 4. Maximal-suppression. Sort by length descending; a candidate is
    // dominated when every occurrence fits inside some accepted candidate's
    // occurrence range.
    candidates_raw.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));

    let mut accepted: Vec<(Vec<String>, Vec<Occ>)> = Vec::new();
    for cand in candidates_raw {
        let dominated = cand.1.iter().all(|o| {
            accepted.iter().any(|(_, acc_occs)| {
                acc_occs.iter().any(|a| {
                    a.container == o.container
                        && a.start <= o.start
                        && o.start + o.length <= a.start + a.length
                })
            })
        });
        if !dominated {
            accepted.push(cand);
        }
    }

    // 5. Build Candidate values with sketches.
    let mut result: Vec<Candidate> = accepted
        .into_iter()
        .map(|(sig, occs)| {
            let length = sig.len();
            let occurrences: Vec<Site> = occs
                .iter()
                .map(|o| Site {
                    container: containers[o.container].id.clone(),
                    start: o.start,
                    end: o.start + o.length,
                })
                .collect();
            let score = length * (occs.len() - 1);
            let first = &occs[0];
            let raw_steps =
                &containers[first.container].raw[first.start..first.start + first.length];
            let sketch = render_sketch(raw_steps);
            Candidate {
                length,
                occurrences,
                score,
                sketch,
            }
        })
        .collect();

    // 6. Ranking. Score desc → length desc → occurrence count desc → site key.
    result.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.length.cmp(&a.length))
            .then_with(|| b.occurrences.len().cmp(&a.occurrences.len()))
            .then_with(|| occurrence_key(a).cmp(&occurrence_key(b)))
    });

    result
}

fn occurrence_key(c: &Candidate) -> Vec<String> {
    c.occurrences
        .iter()
        .map(|s| format!("{}:{}-{}", s.container, s.start, s.end))
        .collect()
}

fn render_sketch(steps: &[&Step]) -> String {
    let mut s = String::new();
    s.push_str("name: extracted-bootstrap\n");
    s.push_str(
        "description: Extracted from duplicated step sequences. Review and parametrize before using.\n",
    );
    s.push_str("runs:\n");
    s.push_str("  using: composite\n");
    s.push_str("  steps:\n");
    for step in steps {
        if let Some(uses) = &step.uses {
            let uses_str = match uses {
                // Local actions must be referenced as `./<path>` inside a
                // composite to satisfy the GitHub Actions spec.
                UsesRef::LocalAction(ActionId(p)) => format!("./{p}"),
                UsesRef::External {
                    owner,
                    repo,
                    subpath,
                    gitref,
                } => match subpath {
                    Some(sub) => format!("{owner}/{repo}/{sub}@{gitref}"),
                    None => format!("{owner}/{repo}@{gitref}"),
                },
                UsesRef::Docker(d) => format!("docker://{}", d.display_str()),
                // `LocalWorkflow` is filtered out by `normalize_step` (it
                // returns `None`), which acts as a wall in window enumeration,
                // so no accepted candidate can contain a reusable-workflow
                // step. This arm exists only to keep the match exhaustive.
                UsesRef::LocalWorkflow(_) => continue,
            };
            s.push_str(&format!("    - uses: {uses_str}\n"));
            if !step.with.is_empty() {
                s.push_str("      # TODO: parametrize via inputs:\n");
            }
        } else if let Some(run) = &step.run {
            // Use the explicit shell if set; fall back to "bash" until Issue #8
            // (inherited defaults) is implemented.
            let shell = step.shell.as_deref().unwrap_or("bash");
            s.push_str(&format!("    - shell: {shell}\n"));
            s.push_str("      run: |\n");
            for line in run.lines() {
                s.push_str(&format!("        {line}\n"));
            }
        }
    }
    s
}

pub fn render_json(candidates: &[Candidate]) -> Result<String> {
    Ok(serde_json::to_string_pretty(candidates)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{EventKind, Job, JobId, SourcePos, TriggerSpec, Workflow, WorkflowId};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn step(index: usize, uses: Option<UsesRef>, run: Option<String>) -> Step {
        Step {
            index,
            id: None,
            name: None,
            uses,
            run,
            if_expr: None,
            with: BTreeMap::new(),
            env: BTreeMap::new(),
            shell: None,
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            annotations: Vec::new(),
        }
    }

    fn external(owner: &str, repo: &str, gitref: &str) -> UsesRef {
        UsesRef::External {
            owner: owner.into(),
            repo: repo.into(),
            subpath: None,
            gitref: gitref.into(),
        }
    }

    fn bootstrap_steps() -> Vec<Step> {
        vec![
            step(0, Some(external("actions", "checkout", "v4")), None),
            step(1, Some(external("actions", "setup-node", "v4")), None),
            step(2, None, Some("npm ci".into())),
            step(3, Some(external("actions", "cache", "v4")), None),
        ]
    }

    fn workflow_with_bootstrap(id: &str, tail: Vec<Step>) -> Workflow {
        let mut steps = bootstrap_steps();
        steps.extend(tail);
        workflow_with_bootstrap_steps(id, steps)
    }

    fn workflow_with_bootstrap_steps(id: &str, steps: Vec<Step>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![Job {
                id: JobId("build".into()),
                workflow: WorkflowId(id.into()),
                needs: vec![],
                permissions: None,
                steps,
                calls_workflow: None,
                runs_on: None,
                outputs: BTreeMap::new(),
                source: SourcePos {
                    file: PathBuf::new(),
                    line: None,
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

    fn three_bootstrap_ir() -> Ir {
        Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                workflow_with_bootstrap(
                    ".github/workflows/a.yml",
                    vec![step(4, None, Some("npm test".into()))],
                ),
                workflow_with_bootstrap(
                    ".github/workflows/b.yml",
                    vec![step(4, None, Some("npm run lint".into()))],
                ),
                workflow_with_bootstrap(
                    ".github/workflows/c.yml",
                    vec![step(4, None, Some("npm run build".into()))],
                ),
            ],
            actions: vec![],
            external_actions: vec![],
        }
    }

    #[test]
    fn happy_path_emits_one_4step_candidate() {
        let ir = three_bootstrap_ir();
        let candidates = find_candidates(&ir, 3, 2);
        assert_eq!(
            candidates.len(),
            1,
            "expected 1 candidate, got {candidates:#?}"
        );
        let c = &candidates[0];
        assert_eq!(c.length, 4);
        assert_eq!(c.occurrences.len(), 3);
        assert!(
            c.sketch.contains("actions/checkout@v4"),
            "sketch missing checkout: {}",
            c.sketch
        );
        assert!(
            c.sketch.contains("npm ci"),
            "sketch missing npm ci: {}",
            c.sketch
        );
        assert!(
            c.sketch.contains("using: composite"),
            "sketch missing composite header: {}",
            c.sketch
        );
    }

    #[test]
    fn maximal_suppression_drops_length3_subsequences() {
        let ir = three_bootstrap_ir();
        let candidates = find_candidates(&ir, 3, 2);
        assert!(
            candidates.iter().all(|c| c.length == 4),
            "no candidate should have length 3, got {candidates:#?}"
        );
    }

    #[test]
    fn local_action_sketch_uses_dot_slash_prefix() {
        // Three workflows that share a 3-step bootstrap whose middle step is a
        // local action. The sketch must address it as `./<path>` so the
        // generated composite is valid.
        let local_action = || {
            step(
                1,
                Some(UsesRef::LocalAction(ActionId(
                    ".github/actions/setup".into(),
                ))),
                None,
            )
        };
        let bootstrap = || {
            vec![
                step(0, Some(external("actions", "checkout", "v4")), None),
                local_action(),
                step(2, None, Some("npm ci".into())),
            ]
        };
        let mk = |id: &str, tail: Step| {
            let mut steps = bootstrap();
            steps.push(tail);
            workflow_with_bootstrap_steps(id, steps)
        };
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                mk(
                    ".github/workflows/a.yml",
                    step(3, None, Some("npm test".into())),
                ),
                mk(
                    ".github/workflows/b.yml",
                    step(3, None, Some("npm run lint".into())),
                ),
                mk(
                    ".github/workflows/c.yml",
                    step(3, None, Some("npm run build".into())),
                ),
            ],
            actions: vec![],
            external_actions: vec![],
        };
        let candidates = find_candidates(&ir, 3, 2);
        assert_eq!(
            candidates.len(),
            1,
            "expected 1 candidate, got {candidates:#?}"
        );
        let sketch = &candidates[0].sketch;
        assert!(
            sketch.contains("uses: ./.github/actions/setup"),
            "expected `./` prefix on local-action ref; got sketch:\n{sketch}"
        );
        assert!(
            !sketch.contains("uses: .github/actions/setup"),
            "sketch must not emit a bare local-action path; got:\n{sketch}"
        );
    }

    #[test]
    fn local_workflow_step_walls_off_candidate() {
        // Three workflows whose only common run is interrupted by a reusable
        // workflow call. Reusable workflows cannot live inside a composite,
        // so `normalize_step` walls off `LocalWorkflow` and the duplicate
        // before/after segments are too short to qualify as a candidate.
        let workflow_call = || {
            step(
                2,
                Some(UsesRef::LocalWorkflow(WorkflowId(
                    ".github/workflows/reusable.yml".into(),
                ))),
                None,
            )
        };
        let mk = |id: &str| {
            let steps = vec![
                step(0, Some(external("actions", "checkout", "v4")), None),
                step(1, Some(external("actions", "setup-node", "v4")), None),
                workflow_call(),
                step(3, None, Some("npm ci".into())),
                step(4, Some(external("actions", "cache", "v4")), None),
            ];
            workflow_with_bootstrap_steps(id, steps)
        };
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                mk(".github/workflows/a.yml"),
                mk(".github/workflows/b.yml"),
                mk(".github/workflows/c.yml"),
            ],
            actions: vec![],
            external_actions: vec![],
        };
        let candidates = find_candidates(&ir, 3, 2);
        // No accepted candidate may include the reusable-workflow signature,
        // and no sketch may emit a `uses: ./.github/workflows/...` line.
        for c in &candidates {
            assert!(
                !c.sketch.contains("reusable.yml"),
                "candidate sketch must not include the reusable workflow:\n{}",
                c.sketch
            );
        }
        // With a 3-step minimum, the 2-step head and 2-step tail on either
        // side of the wall cannot form a candidate, so none should be
        // emitted from this fixture.
        assert!(
            candidates.is_empty(),
            "expected no candidate when only common sequences are split by a reusable-workflow wall, got {candidates:#?}"
        );
    }

    #[test]
    fn shell_preamble_skip() {
        assert!(is_shell_preamble("set -e"));
        assert!(is_shell_preamble("set -eu"));
        assert!(is_shell_preamble("set -ue"));
        assert!(is_shell_preamble("set -euo pipefail"));
        assert!(!is_shell_preamble("set -x"));
        assert!(!is_shell_preamble("set"));
        assert!(!is_shell_preamble("npm ci"));

        let body = "#!/usr/bin/env bash\nset -euo pipefail\nnpm test\n";
        assert_eq!(first_meaningful_line(body), Some("npm test".to_string()));
    }

    /// Cluster of exactly 2 identical bootstrap sequences must produce one
    /// candidate with two occurrences and the score formula `length * (n-1)`.
    #[test]
    fn cluster_of_two_emits_single_candidate_with_two_occurrences() {
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                workflow_with_bootstrap(
                    ".github/workflows/a.yml",
                    vec![step(4, None, Some("npm test".into()))],
                ),
                workflow_with_bootstrap(
                    ".github/workflows/b.yml",
                    vec![step(4, None, Some("npm run lint".into()))],
                ),
            ],
            actions: vec![],
            external_actions: vec![],
        };
        let candidates = find_candidates(&ir, 3, 2);
        assert_eq!(candidates.len(), 1, "{candidates:#?}");
        assert_eq!(candidates[0].length, 4);
        assert_eq!(candidates[0].occurrences.len(), 2);
        assert_eq!(candidates[0].score, 4); // length(4) * (n(2) - 1)
    }

    /// Cluster of exactly 3: same shape as the existing happy path. Score
    /// scales with `n - 1`, so 3 occurrences score 8 (length 4 * 2).
    #[test]
    fn cluster_of_three_score_scales_with_occurrences() {
        let ir = three_bootstrap_ir();
        let candidates = find_candidates(&ir, 3, 2);
        assert_eq!(candidates.len(), 1, "{candidates:#?}");
        assert_eq!(candidates[0].occurrences.len(), 3);
        assert_eq!(candidates[0].score, 8);
    }

    /// Cluster of 4+: four identical bootstraps must collapse into one
    /// candidate with four occurrences and a higher score than the
    /// 3-occurrence case.
    #[test]
    fn cluster_of_four_or_more_collapses_into_one_candidate() {
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                workflow_with_bootstrap(
                    ".github/workflows/a.yml",
                    vec![step(4, None, Some("npm test".into()))],
                ),
                workflow_with_bootstrap(
                    ".github/workflows/b.yml",
                    vec![step(4, None, Some("npm run lint".into()))],
                ),
                workflow_with_bootstrap(
                    ".github/workflows/c.yml",
                    vec![step(4, None, Some("npm run build".into()))],
                ),
                workflow_with_bootstrap(
                    ".github/workflows/d.yml",
                    vec![step(4, None, Some("npm run docs".into()))],
                ),
            ],
            actions: vec![],
            external_actions: vec![],
        };
        let candidates = find_candidates(&ir, 3, 2);
        assert_eq!(candidates.len(), 1, "{candidates:#?}");
        assert_eq!(candidates[0].length, 4);
        assert_eq!(candidates[0].occurrences.len(), 4);
        // length(4) * (n(4) - 1) = 12, must exceed the 3-occurrence score (8).
        assert_eq!(candidates[0].score, 12);
    }

    /// Below `min_occurrences` (similarity threshold boundary): with `n = 3`
    /// required and only 2 matching containers, no candidate is emitted.
    #[test]
    fn below_min_occurrences_threshold_emits_no_candidate() {
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                workflow_with_bootstrap(
                    ".github/workflows/a.yml",
                    vec![step(4, None, Some("npm test".into()))],
                ),
                workflow_with_bootstrap(
                    ".github/workflows/b.yml",
                    vec![step(4, None, Some("npm run lint".into()))],
                ),
            ],
            actions: vec![],
            external_actions: vec![],
        };
        let candidates = find_candidates(&ir, 3, 3);
        assert!(
            candidates.is_empty(),
            "fewer occurrences than min_occurrences must yield nothing: {candidates:#?}"
        );
    }

    /// Below `min_length`: a bootstrap of length 4 cannot produce a candidate
    /// when `min_length = 5`. Boundary case for the length filter.
    #[test]
    fn below_min_length_threshold_emits_no_candidate() {
        let ir = three_bootstrap_ir();
        let candidates = find_candidates(&ir, 5, 2);
        assert!(
            candidates.is_empty(),
            "min_length above container length must yield nothing: {candidates:#?}"
        );
    }

    /// Steps that differ only in `with:` keys must still cluster, because
    /// `normalize_step` ignores `with:` per the design doc.
    #[test]
    fn steps_differing_only_in_with_keys_still_cluster() {
        // Build three workflows whose checkout step has different `with:` maps
        // and whose tail step diverges. The bootstrap (with the differing
        // `with:`) should still cluster as a length-4 candidate.
        let make_checkout_with = |fetch_depth: &str| {
            let mut s = step(0, Some(external("actions", "checkout", "v4")), None);
            s.with.insert("fetch-depth".into(), fetch_depth.to_string());
            s
        };

        let mut ir = three_bootstrap_ir();
        // Replace the first step in each workflow with a `with:`-bearing checkout.
        for (i, depth) in ["1", "0", "2"].iter().enumerate() {
            ir.workflows[i].jobs[0].steps[0] = make_checkout_with(depth);
        }

        let candidates = find_candidates(&ir, 3, 2);
        assert_eq!(
            candidates.len(),
            1,
            "with: differences must not break clustering: {candidates:#?}"
        );
        assert_eq!(candidates[0].length, 4);
        assert_eq!(candidates[0].occurrences.len(), 3);
        // The sketch must annotate the parametrization TODO when `with:` is
        // present on the matched step.
        assert!(
            candidates[0].sketch.contains("parametrize via inputs"),
            "expected TODO comment in sketch when `with:` is present:\n{}",
            candidates[0].sketch
        );
    }

    /// Steps with neither `uses:` nor `run:` are non-determinable:
    /// `normalize_step` returns `None`, walling off any window that would
    /// cross them. The tail (3 steps after the wall) is identical across
    /// workflows and must produce a candidate; no candidate may straddle
    /// the wall.
    #[test]
    fn non_determinable_steps_act_as_walls() {
        // A step with neither `uses:` nor `run:` is non-determinable.
        let non_determinable = || step(1, None, None);

        let mk = |id: &str, head_run: &str| {
            let steps = vec![
                step(0, None, Some(head_run.to_string())),
                non_determinable(),
                step(2, Some(external("actions", "checkout", "v4")), None),
                step(3, Some(external("actions", "setup-node", "v4")), None),
                step(4, None, Some("npm ci".into())),
            ];
            workflow_with_bootstrap_steps(id, steps)
        };
        let ir = Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows: vec![
                mk(".github/workflows/a.yml", "echo a"),
                mk(".github/workflows/b.yml", "echo b"),
                mk(".github/workflows/c.yml", "echo c"),
            ],
            actions: vec![],
            external_actions: vec![],
        };
        // With min_length=3, the head (1 step + wall) cannot produce a
        // candidate; only the 3-step tail (after the wall) qualifies.
        let candidates = find_candidates(&ir, 3, 2);
        assert_eq!(
            candidates.len(),
            1,
            "expected single tail-side candidate: {candidates:#?}"
        );
        assert_eq!(candidates[0].length, 3);
        for site in &candidates[0].occurrences {
            // The candidate must start AFTER the wall (index 1), i.e. start
            // index ≥ 2. If clustering crossed the wall, start would be 0.
            assert!(
                site.start >= 2,
                "no occurrence may straddle the non-determinable wall: {site:?}"
            );
        }
    }

    /// `min_occurrences = 1` is rejected by the API contract — fewer than 2
    /// occurrences cannot represent duplication.
    #[test]
    fn min_occurrences_below_two_rejected() {
        let ir = three_bootstrap_ir();
        let candidates = find_candidates(&ir, 3, 1);
        assert!(
            candidates.is_empty(),
            "min_occurrences < 2 must short-circuit: {candidates:#?}"
        );

        // min_length = 0 is also rejected.
        let candidates = find_candidates(&ir, 0, 2);
        assert!(
            candidates.is_empty(),
            "min_length = 0 must short-circuit: {candidates:#?}"
        );
    }
}
