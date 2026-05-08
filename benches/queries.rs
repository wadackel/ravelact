use criterion::{black_box, criterion_group, criterion_main, Criterion};
use ravelact::ir::*;
use ravelact::query::{impact, orphans, trace};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// IR construction helpers (mirrored from src/query/impact.rs::tests)
// ---------------------------------------------------------------------------

fn empty_step(index: usize, uses: Option<UsesRef>) -> Step {
    Step {
        index,
        id: None,
        name: None,
        uses,
        run: None,
        if_expr: None,
        with: Default::default(),
        env: Default::default(),
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

fn job(workflow_id: &str, id: &str, steps: Vec<Step>, calls: Option<CallsWorkflow>) -> Job {
    Job {
        id: JobId(id.into()),
        workflow: WorkflowId(workflow_id.into()),
        needs: vec![],
        permissions: None,
        steps,
        calls_workflow: calls,
        runs_on: None,
        outputs: Default::default(),
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

fn push_trigger() -> TriggerSpec {
    TriggerSpec::bare(EventKind::Push)
}

fn workflow_call_trigger() -> TriggerSpec {
    TriggerSpec::bare(EventKind::WorkflowCall)
}

fn local_call(target_id: &str) -> CallsWorkflow {
    CallsWorkflow {
        workflow_ref: WorkflowRef::Local(WorkflowId(target_id.into())),
        with: Default::default(),
        secrets: SecretsPass::None,
    }
}

// ---------------------------------------------------------------------------
// Synthetic fan-out estate
//
// Tree shape: `entries` entry-point workflows, each branching `fanout`-way per
// level for `depth` levels. All non-root workflows are reusable
// (`workflow_call`-only). Total workflow count = entries * (1 + fanout +
// fanout^2 + ... + fanout^depth).
//
// With (entries=10, fanout=5, depth=4) the tree contains:
//     10 * (1 + 5 + 25 + 125 + 625) = 7810 workflows
//
// We deliberately pick a configuration that stresses the linear-find hot path
// (`iter().find(...)` over `ir.workflows`) hard enough to make the O(N²)
// blowup measurable above criterion's noise floor while keeping bench wall
// time reasonable for local runs.
// ---------------------------------------------------------------------------

fn build_synthetic_ir(entries: usize, fanout: usize, depth: usize) -> Ir {
    let mut workflows: Vec<Workflow> = Vec::new();

    fn id_for(entry: usize, path: &[usize]) -> String {
        let mut s = format!(".github/workflows/wf-{entry:03}");
        for p in path {
            s.push_str(&format!("-{p}"));
        }
        s.push_str(".yml");
        s
    }

    fn add_subtree(
        workflows: &mut Vec<Workflow>,
        entry: usize,
        path: &mut Vec<usize>,
        fanout: usize,
        depth: usize,
        is_root: bool,
    ) {
        let my_id = id_for(entry, path);
        let trigger = if is_root {
            push_trigger()
        } else {
            workflow_call_trigger()
        };

        // Build child IDs first so we can fan out via job-level `calls_workflow`.
        let mut child_ids: Vec<String> = Vec::new();
        if depth > 0 {
            for k in 0..fanout {
                path.push(k);
                child_ids.push(id_for(entry, path));
                path.pop();
            }
        }

        // One job per child to give BFS something to do; if no children, a
        // single empty job keeps the workflow shape consistent.
        let jobs: Vec<Job> = if child_ids.is_empty() {
            vec![job(&my_id, "leaf", vec![empty_step(0, None)], None)]
        } else {
            child_ids
                .iter()
                .enumerate()
                .map(|(i, cid)| {
                    job(
                        &my_id,
                        &format!("call-{i}"),
                        vec![empty_step(0, None)],
                        Some(local_call(cid)),
                    )
                })
                .collect()
        };

        workflows.push(workflow(&my_id, vec![trigger], jobs));

        if depth > 0 {
            for k in 0..fanout {
                path.push(k);
                add_subtree(workflows, entry, path, fanout, depth - 1, false);
                path.pop();
            }
        }
    }

    for entry in 0..entries {
        let mut path: Vec<usize> = Vec::new();
        add_subtree(&mut workflows, entry, &mut path, fanout, depth, true);
    }

    Ir {
        schema_version: 1,
        root: PathBuf::from("/tmp/bench"),
        workflows,
        actions: Vec::new(),
        external_actions: Vec::new(),
    }
}

fn entry_path(entry: usize) -> String {
    format!(".github/workflows/wf-{entry:03}.yml")
}

// ---------------------------------------------------------------------------
// Benches
// ---------------------------------------------------------------------------

fn bench_orphans(c: &mut Criterion) {
    let ir = build_synthetic_ir(10, 5, 4);
    c.bench_function("orphans/10x5x4", |b| {
        b.iter(|| orphans::orphans(black_box(&ir)))
    });
}

fn bench_trace(c: &mut Criterion) {
    let ir = build_synthetic_ir(10, 5, 4);
    c.bench_function("trace/push/10x5x4", |b| {
        b.iter(|| -> Vec<trace::TraceEntry> {
            trace::trace(
                black_box(&ir),
                black_box("push"),
                black_box(&[]),
                black_box(&[]),
                black_box(&[]),
                black_box(&[]),
            )
        })
    });
}

fn bench_impact(c: &mut Criterion) {
    let ir = build_synthetic_ir(10, 5, 4);
    // Seed a single entry workflow so reverse_bfs has a non-trivial visited
    // set to feed into the post-traversal `find` lookup.
    let files: Vec<String> = vec![entry_path(0)];
    c.bench_function("impact/10x5x4", |b| {
        b.iter(|| impact::impact(black_box(&ir), black_box(&files)))
    });
}

criterion_group!(benches, bench_orphans, bench_trace, bench_impact);
criterion_main!(benches);
