//! Near-duplicate workflow detection (Issue #10, Phase 3 `suggest dedup`).
//!
//! Extracts a feature set per workflow (triggers / job ids / step `uses` /
//! whitespace-tokenized step `run` body), scores every pair via weighted
//! Jaccard, links pairs above the configured threshold using single-linkage
//! union-find, and surfaces clusters of size ≥ 2 with a per-cluster
//! representative + diff summary.

use crate::ir::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct DedupCluster {
    pub cluster_index: usize,
    pub representative: WorkflowId,
    pub members: Vec<DedupMember>,
    pub common_uses: Vec<String>,
    pub divergent_uses: Vec<String>,
    pub triggers_differ: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DedupMember {
    pub workflow: WorkflowId,
    pub similarity_to_representative: f32,
}

struct Features {
    triggers: BTreeSet<String>,
    job_ids: BTreeSet<String>,
    uses: BTreeSet<String>,
    run_tokens: BTreeSet<String>,
}

const W_TRIGGERS: f32 = 0.15;
const W_JOBS: f32 = 0.10;
const W_USES: f32 = 0.40;
const W_RUNS: f32 = 0.35;

fn extract_features(wf: &Workflow) -> Features {
    let triggers: BTreeSet<String> = wf
        .triggers
        .iter()
        .map(|t| t.event_name().to_string())
        .collect();
    let job_ids: BTreeSet<String> = wf.jobs.iter().map(|j| j.id.0.clone()).collect();

    let mut uses: BTreeSet<String> = BTreeSet::new();
    let mut run_concat = String::new();
    for j in &wf.jobs {
        for step in &j.steps {
            if let Some(u) = &step.uses {
                uses.insert(normalize_uses(u));
            }
            if let Some(r) = &step.run {
                if !run_concat.is_empty() {
                    run_concat.push('\n');
                }
                run_concat.push_str(r);
            }
        }
    }
    let run_tokens: BTreeSet<String> = run_concat
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    Features {
        triggers,
        job_ids,
        uses,
        run_tokens,
    }
}

fn normalize_uses(u: &UsesRef) -> String {
    match u {
        UsesRef::LocalWorkflow(WorkflowId(p)) => format!("local-wf:{p}"),
        UsesRef::LocalAction(ActionId(p)) => format!("local-action:{p}"),
        UsesRef::External {
            owner,
            repo,
            subpath,
            ..
        } => match subpath {
            Some(sub) => format!("ext:{owner}/{repo}/{sub}"),
            None => format!("ext:{owner}/{repo}"),
        },
        UsesRef::Docker(d) => format!("docker:{}", d.display_str()),
    }
}

fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let inter = a.intersection(b).count() as f32;
    let uni = a.union(b).count() as f32;
    inter / uni
}

fn similarity(a: &Features, b: &Features) -> f32 {
    W_TRIGGERS * jaccard(&a.triggers, &b.triggers)
        + W_JOBS * jaccard(&a.job_ids, &b.job_ids)
        + W_USES * jaccard(&a.uses, &b.uses)
        + W_RUNS * jaccard(&a.run_tokens, &b.run_tokens)
}

fn uf_find(parent: &mut [usize], x: usize) -> usize {
    let mut root = x;
    while parent[root] != root {
        root = parent[root];
    }
    let mut cur = x;
    while parent[cur] != root {
        let next = parent[cur];
        parent[cur] = root;
        cur = next;
    }
    root
}

fn uf_union(parent: &mut [usize], a: usize, b: usize) {
    let ra = uf_find(parent, a);
    let rb = uf_find(parent, b);
    if ra != rb {
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        parent[hi] = lo;
    }
}

pub fn dedup(ir: &Ir, threshold: f32) -> Vec<DedupCluster> {
    let n = ir.workflows.len();
    if n < 2 {
        return Vec::new();
    }

    let features: Vec<Features> = ir.workflows.iter().map(extract_features).collect();

    let mut parent: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in (i + 1)..n {
            let s = similarity(&features[i], &features[j]);
            if s >= threshold {
                uf_union(&mut parent, i, j);
            }
        }
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = uf_find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }

    let mut clusters: Vec<DedupCluster> = Vec::new();
    for (_root, mut indices) in groups.into_iter() {
        if indices.len() < 2 {
            continue;
        }
        indices.sort_by(|a, b| ir.workflows[*a].id.0.cmp(&ir.workflows[*b].id.0));
        let rep_idx = indices[0];
        let rep_wf = &ir.workflows[rep_idx];

        let members: Vec<DedupMember> = indices[1..]
            .iter()
            .map(|&idx| DedupMember {
                workflow: ir.workflows[idx].id.clone(),
                similarity_to_representative: similarity(&features[rep_idx], &features[idx]),
            })
            .collect();

        let mut iter = indices.iter().map(|&idx| &features[idx].uses);
        let mut common: BTreeSet<String> = iter.next().unwrap().clone();
        for s in iter {
            common = common.intersection(s).cloned().collect();
        }
        let mut union_set: BTreeSet<String> = BTreeSet::new();
        for &idx in &indices {
            for u in &features[idx].uses {
                union_set.insert(u.clone());
            }
        }
        let divergent: BTreeSet<String> = union_set.difference(&common).cloned().collect();

        let common_uses: Vec<String> = common.into_iter().collect();
        let divergent_uses: Vec<String> = divergent.into_iter().collect();

        let first_triggers = &features[indices[0]].triggers;
        let triggers_differ = indices
            .iter()
            .skip(1)
            .any(|&idx| &features[idx].triggers != first_triggers);

        clusters.push(DedupCluster {
            cluster_index: 0,
            representative: rep_wf.id.clone(),
            members,
            common_uses,
            divergent_uses,
            triggers_differ,
        });
    }

    clusters.sort_by(|a, b| a.representative.0.cmp(&b.representative.0));
    for (i, c) in clusters.iter_mut().enumerate() {
        c.cluster_index = i;
    }
    clusters
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn step(index: usize, uses: Option<UsesRef>, run: Option<&str>) -> Step {
        Step {
            index,
            id: None,
            name: None,
            uses,
            run: run.map(|s| s.to_string()),
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

    fn workflow(id: &str, triggers: Vec<TriggerSpec>, jobs: Vec<Job>) -> Workflow {
        Workflow {
            id: WorkflowId(id.into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            run_name: None,
            triggers,
            jobs,
            permissions: None,
            defaults: None,
            env: Default::default(),
            concurrency: None,
            annotations: Vec::new(),
        }
    }

    fn job(workflow_id: &str, id: &str, steps: Vec<Step>) -> Job {
        Job {
            id: JobId(id.into()),
            workflow: WorkflowId(workflow_id.into()),
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
        }
    }

    fn ir_with(workflows: Vec<Workflow>) -> Ir {
        Ir {
            schema_version: 1,
            root: PathBuf::from("/tmp/test"),
            workflows,
            actions: vec![],
            external_actions: vec![],
        }
    }

    fn push() -> TriggerSpec {
        TriggerSpec::bare(EventKind::Push)
    }
    fn pr() -> TriggerSpec {
        TriggerSpec::bare(EventKind::PullRequest)
    }
    fn dispatch() -> TriggerSpec {
        TriggerSpec::bare(EventKind::WorkflowDispatch)
    }

    fn ext(owner: &str, repo: &str, gitref: &str) -> UsesRef {
        UsesRef::External {
            owner: owner.into(),
            repo: repo.into(),
            subpath: None,
            gitref: gitref.into(),
        }
    }

    fn clone_with_id(src: &Workflow, new_id: &str) -> Workflow {
        let mut clone = src.clone();
        clone.id = WorkflowId(new_id.into());
        for j in &mut clone.jobs {
            j.workflow = clone.id.clone();
        }
        clone
    }

    // --- Feature-extraction / metric properties -----------------------------------

    #[test]
    fn identical_workflows_yield_similarity_one() {
        let a = workflow(
            ".github/workflows/a.yml",
            vec![push(), dispatch()],
            vec![job(
                ".github/workflows/a.yml",
                "build",
                vec![
                    step(0, Some(ext("actions", "checkout", "v4")), None),
                    step(1, None, Some("echo hello world")),
                ],
            )],
        );
        let b = clone_with_id(&a, ".github/workflows/b.yml");
        let ir = ir_with(vec![a, b]);
        let result = dedup(&ir, 0.8);
        assert_eq!(result.len(), 1);
        let cluster = &result[0];
        assert_eq!(cluster.representative.0, ".github/workflows/a.yml");
        assert_eq!(cluster.members.len(), 1);
        assert!(
            (cluster.members[0].similarity_to_representative - 1.0).abs() < 1e-6,
            "got {}",
            cluster.members[0].similarity_to_representative
        );
    }

    #[test]
    fn trigger_only_diff_still_clusters_at_default_threshold() {
        // A: push, B: pr — disjoint trigger sets → J(triggers) = 0
        // jobs/uses/runs identical → 0 + 0.10 + 0.40 + 0.35 = 0.85 ≥ 0.8
        let a = workflow(
            ".github/workflows/a.yml",
            vec![push()],
            vec![job(
                ".github/workflows/a.yml",
                "build",
                vec![
                    step(0, Some(ext("actions", "checkout", "v4")), None),
                    step(1, None, Some("echo hi")),
                ],
            )],
        );
        let mut b = clone_with_id(&a, ".github/workflows/b.yml");
        b.triggers = vec![pr()];
        let ir = ir_with(vec![a, b]);
        let result = dedup(&ir, 0.8);
        assert_eq!(result.len(), 1);
        assert!(result[0].triggers_differ);
    }

    #[test]
    fn very_different_workflows_dont_cluster() {
        let a = workflow(
            ".github/workflows/a.yml",
            vec![push()],
            vec![job(
                ".github/workflows/a.yml",
                "build",
                vec![
                    step(0, Some(ext("actions", "checkout", "v4")), None),
                    step(1, None, Some("yarn build all things")),
                ],
            )],
        );
        let b = workflow(
            ".github/workflows/b.yml",
            vec![pr()],
            vec![job(
                ".github/workflows/b.yml",
                "lint",
                vec![
                    step(0, Some(ext("actions", "setup-node", "v4")), None),
                    step(1, None, Some("yarn lint --strict")),
                ],
            )],
        );
        let ir = ir_with(vec![a, b]);
        let result = dedup(&ir, 0.8);
        assert!(result.is_empty(), "expected no clusters, got {result:?}");
    }

    #[test]
    fn run_tokens_dim_is_one_when_token_sets_match() {
        let a = workflow(
            ".github/workflows/a.yml",
            vec![push()],
            vec![job(
                ".github/workflows/a.yml",
                "build",
                vec![step(0, None, Some("echo hello world"))],
            )],
        );
        let mut b = clone_with_id(&a, ".github/workflows/b.yml");
        // reorder tokens via newline — token *set* identical, token *order* differs
        b.jobs[0].steps[0].run = Some("hello\nworld\necho".into());
        let ir = ir_with(vec![a, b]);
        let result = dedup(&ir, 1.0);
        assert_eq!(result.len(), 1, "should cluster at threshold 1.0 exactly");
    }

    // --- Clustering algorithm properties -----------------------------------------

    #[test]
    fn single_linkage_transitive_chain() {
        // A↔B (high), B↔C (high), A↔C (below threshold).
        // Three workflows differ only in their run-token sets.
        let mk = |id: &str, run_tokens: &str| -> Workflow {
            workflow(
                id,
                vec![push()],
                vec![job(
                    id,
                    "build",
                    vec![
                        step(0, Some(ext("actions", "checkout", "v4")), None),
                        step(1, Some(ext("actions", "setup-node", "v4")), None),
                        step(2, None, Some(run_tokens)),
                    ],
                )],
            )
        };
        // tokens: A={x,y,z}, B={x,y}, C={y}
        // J(A,B)=2/3, J(B,C)=1/2, J(A,C)=1/3
        // similarity = 0.65 (base from triggers/jobs/uses fully identical) + 0.35 * J(runs)
        // = 0.883 / 0.825 / 0.767
        let a = mk(".github/workflows/a.yml", "x y z");
        let b = mk(".github/workflows/b.yml", "x y");
        let c = mk(".github/workflows/c.yml", "y");
        let ir = ir_with(vec![a, b, c]);
        let result = dedup(&ir, 0.8);
        assert_eq!(result.len(), 1, "expected single transitive cluster");
        let cluster = &result[0];
        assert_eq!(cluster.representative.0, ".github/workflows/a.yml");
        assert_eq!(cluster.members.len(), 2);
        let member_ids: Vec<&str> = cluster
            .members
            .iter()
            .map(|m| m.workflow.0.as_str())
            .collect();
        assert_eq!(
            member_ids,
            vec![".github/workflows/b.yml", ".github/workflows/c.yml"]
        );
    }

    #[test]
    fn singleton_clusters_are_omitted() {
        let a = workflow(
            ".github/workflows/a.yml",
            vec![push()],
            vec![job(
                ".github/workflows/a.yml",
                "build",
                vec![step(0, Some(ext("actions", "checkout", "v4")), None)],
            )],
        );
        let b = workflow(
            ".github/workflows/b.yml",
            vec![pr()],
            vec![job(
                ".github/workflows/b.yml",
                "lint",
                vec![step(0, Some(ext("actions", "setup-node", "v4")), None)],
            )],
        );
        let ir = ir_with(vec![a, b]);
        let result = dedup(&ir, 0.8);
        assert!(result.is_empty());
    }

    #[test]
    fn representative_is_lexicographically_smallest() {
        let mk = |id: &str| -> Workflow {
            workflow(
                id,
                vec![push()],
                vec![job(
                    id,
                    "build",
                    vec![step(0, Some(ext("actions", "checkout", "v4")), None)],
                )],
            )
        };
        let zebra = mk(".github/workflows/zebra.yml");
        let alpha = mk(".github/workflows/alpha.yml");
        let mango = mk(".github/workflows/mango.yml");
        // Insert in non-sorted order to ensure rep selection is by id, not insertion order.
        let ir = ir_with(vec![zebra, alpha, mango]);
        let result = dedup(&ir, 0.8);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].representative.0, ".github/workflows/alpha.yml");
        let member_ids: Vec<&str> = result[0]
            .members
            .iter()
            .map(|m| m.workflow.0.as_str())
            .collect();
        assert_eq!(
            member_ids,
            vec![".github/workflows/mango.yml", ".github/workflows/zebra.yml"]
        );
    }

    #[test]
    fn threshold_eq_similarity_is_inclusive() {
        // Identical workflows → similarity 1.0; threshold 1.0 is inclusive.
        let a = workflow(
            ".github/workflows/a.yml",
            vec![push()],
            vec![job(
                ".github/workflows/a.yml",
                "build",
                vec![step(0, Some(ext("actions", "checkout", "v4")), None)],
            )],
        );
        let b = clone_with_id(&a, ".github/workflows/b.yml");
        let ir = ir_with(vec![a, b]);
        let result = dedup(&ir, 1.0);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn external_action_ref_is_stripped_for_normalization() {
        let a = workflow(
            ".github/workflows/a.yml",
            vec![push()],
            vec![job(
                ".github/workflows/a.yml",
                "build",
                vec![step(0, Some(ext("actions", "checkout", "v4")), None)],
            )],
        );
        let mut b = clone_with_id(&a, ".github/workflows/b.yml");
        b.jobs[0].steps[0].uses = Some(ext("actions", "checkout", "v6"));
        let ir = ir_with(vec![a, b]);
        let result = dedup(&ir, 1.0);
        assert_eq!(
            result.len(),
            1,
            "@v4 and @v6 should normalize to the same use key"
        );
    }

    #[test]
    fn output_is_deterministic_across_runs() {
        let mk = |id: &str, tokens: &str| -> Workflow {
            workflow(
                id,
                vec![push()],
                vec![job(
                    id,
                    "build",
                    vec![
                        step(0, Some(ext("actions", "checkout", "v4")), None),
                        step(1, None, Some(tokens)),
                    ],
                )],
            )
        };
        let ir = ir_with(vec![
            mk(".github/workflows/c.yml", "echo c"),
            mk(".github/workflows/a.yml", "echo a"),
            mk(".github/workflows/b.yml", "echo b"),
        ]);
        let r1 = dedup(&ir, 0.8);
        let r2 = dedup(&ir, 0.8);
        assert_eq!(r1.len(), r2.len());
        for (c1, c2) in r1.iter().zip(r2.iter()) {
            assert_eq!(c1.representative.0, c2.representative.0);
            let m1: Vec<&str> = c1.members.iter().map(|m| m.workflow.0.as_str()).collect();
            let m2: Vec<&str> = c2.members.iter().map(|m| m.workflow.0.as_str()).collect();
            assert_eq!(m1, m2);
            assert_eq!(c1.common_uses, c2.common_uses);
            assert_eq!(c1.divergent_uses, c2.divergent_uses);
            assert_eq!(c1.triggers_differ, c2.triggers_differ);
        }
    }

    #[test]
    fn triggers_differ_false_when_all_members_share_triggers() {
        let mk = |id: &str| -> Workflow {
            workflow(
                id,
                vec![push(), dispatch()],
                vec![job(
                    id,
                    "build",
                    vec![step(0, Some(ext("actions", "checkout", "v4")), None)],
                )],
            )
        };
        let ir = ir_with(vec![
            mk(".github/workflows/a.yml"),
            mk(".github/workflows/b.yml"),
        ]);
        let result = dedup(&ir, 0.8);
        assert_eq!(result.len(), 1);
        assert!(!result[0].triggers_differ);
    }

    #[test]
    fn divergent_uses_is_union_minus_intersection() {
        // 4 uses each, 3 common → J(uses) = 3/5 = 0.6, similarity = 0.84 (clusters at 0.8).
        let a = workflow(
            ".github/workflows/a.yml",
            vec![push()],
            vec![job(
                ".github/workflows/a.yml",
                "build",
                vec![
                    step(0, Some(ext("actions", "checkout", "v4")), None),
                    step(1, Some(ext("actions", "setup-node", "v4")), None),
                    step(2, Some(ext("actions", "cache", "v4")), None),
                    step(3, Some(ext("actions", "upload-artifact", "v4")), None),
                    step(4, None, Some("echo same tokens here")),
                ],
            )],
        );
        let mut b = clone_with_id(&a, ".github/workflows/b.yml");
        // Replace upload-artifact with download-artifact → 1 divergent each side, 3 common.
        b.jobs[0].steps[3].uses = Some(ext("actions", "download-artifact", "v4"));
        let ir = ir_with(vec![a, b]);
        let result = dedup(&ir, 0.8);
        assert_eq!(result.len(), 1);
        let c = &result[0];
        assert_eq!(
            c.common_uses,
            vec![
                "ext:actions/cache".to_string(),
                "ext:actions/checkout".to_string(),
                "ext:actions/setup-node".to_string(),
            ]
        );
        assert_eq!(
            c.divergent_uses,
            vec![
                "ext:actions/download-artifact".to_string(),
                "ext:actions/upload-artifact".to_string(),
            ]
        );
    }
}
