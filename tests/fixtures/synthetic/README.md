# Synthetic Fixture Suite

End-to-end tests under `tests/e2e.rs` run against the hand-crafted, license-free
workflow estates in this directory. Each fixture targets a specific structural
feature ravelact must analyze; nothing here is derived from any real upstream
project.

## Fixture catalog

| Fixture | Structural feature(s) | Notes |
|---|---|---|
| `workflow-run-chain` | 3-deep `workflow_run` name-match resolution | trigger → middle → downstream + nightly sibling |
| `workflow-call-chain` | `workflow_call` chain depth | caller → leaf |
| `secrets-explicit-chain` | Explicit `secrets:` propagation depth ≥ 2 | caller → mid → leaf |
| `js-action-node16` | JavaScript action manifest (`runs.using: node16`) | minimal `action.yml` |
| `js-action-node24` | JavaScript action manifest (`runs.using: node24`) | minimal `action.yml` |
| `dedup-cluster` | Small dedup cluster baseline | 3 near-identical workflows |
| `minimal-estate` | `push` + `pull_request` triggers, `strategy.matrix.node-version`, canonical checkout/setup-node pair | single workflow |
| `docker-action` | Repository-root `action.yml` with `runs.using: docker` plus a nested `runs.using: composite` action | 4 workflows + 1 root Docker + 1 nested composite |
| `matrix-heavy` | `strategy.matrix` of `os` × `node-version` with `include` / `exclude` clauses | 13 workflows |
| `cross-repo-call` | Internal reusable workflow (`_reusable.yml`) called by ≥ 2 entry-points + cross-repo `workflow_call` to `example-org/shared-workflows` (40-zero SHA, intentionally fictional) | 17 workflows (2 reusable) |
| `nonstandard-composite-path` | Composite action placed outside `.github/workflows/` and `.github/actions/` (here: `.github/build_cache/`) | 11 workflows + 1 composite at the non-standard path |
| `large-estate` | ~28 workflows with mixed triggers and 2 composites; ≥ 7 `update-<lib>.yml` workflows share an `actions/checkout@v4` + `peter-evans/create-pull-request@v7` step pair so the dedup query forms one cluster of update siblings; `permissions:` declarations are diversified (explicit `contents: read, id-token: write` plus omissions) | 28 workflows + 2 composites |
| `mixed-action-types` | All three local action kinds (composite / JavaScript node20 / Docker) co-located, none referenced by the lone workflow — covers per-kind orphans labels and the `actions[].kind` JSON field | 1 workflow + 3 unused local actions |
| `dangling-local-uses` | A step references `./.github/actions/typo` with NO action manifest present — exercises the wiring `DanglingLocalUses` finding and `graph` resilience to unresolved local-action ids (issue #111) | 1 workflow, 0 actions |
| `needs-outputs-conditional` | Cross-job `outputs:` propagation through `needs.<job>.outputs` consumed by downstream `if:` gates | 1 workflow, 3 jobs (detect → build → publish) |
| `dynamic-matrix-from-json` | Dynamic `strategy.matrix` populated by `fromJson(needs.<job>.outputs.<key>)` with `fail-fast: false` and a downstream aggregator job | 1 workflow, 3 jobs (setup → test fan-out → aggregate) |
| `workflow-dispatch-typed-inputs` | `workflow_dispatch.inputs` mixing `type: choice` (with `options:`), `type: environment`, `type: boolean`, and `type: string`; downstream `environment:` keyed by `inputs.target_environment` | 1 workflow, 2 jobs |
| `empty-permissions-drop` | Workflow-level `permissions: {}` deny-all baseline combined with job-level `permissions: {}` drops and a job that selectively re-grants `security-events: write` | 1 workflow, 3 jobs |
| `environment-deployment` | Job `environment:` referencing a deployment environment (both inline-name and `name:` + `url:` forms) plus `permissions.id-token: write` for OIDC trusted publishing | 1 workflow, 2 jobs |
| `services-postgres` | Job-level `services:` containers with `image:`, `ports:`, `options:` (health-check), and `env:` — postgres + redis | 1 workflow, 1 job |
| `composite-invokes-composite` | Composite action whose `runs.steps[].uses` invokes another composite via a relative `./.github/actions/<name>` path (composite chain depth ≥ 2) | 1 workflow + 2 chained composites |
| `concurrency-input-expression` | `concurrency.group` and `concurrency.cancel-in-progress` built from a non-trivial expression involving `inputs.*`, `github.event.*`, and `github.ref` | 1 workflow, 1 job |

## Updating fixtures

After modifying a fixture, run `nix develop -c cargo test --test e2e` to materialize pending snapshots, then refresh with `nix develop -c cargo insta review` (interactive accept/reject). Do not blindly accept — review every diff first.
