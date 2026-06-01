# Development Guide

Reference for building, testing, and maintaining `ravelact` locally.

## Environment

The project ships a [Nix](https://nixos.org/) flake and a [`justfile`](https://just.systems/) for a reproducible toolchain. All canonical recipes run inside the Nix dev shell.

```sh
nix develop                  # enter dev shell (rust toolchain + just + jq + actionlint + zizmor)
exit                         # leave the shell
```

If you prefer to manage your toolchain yourself, plain `cargo` works too — but CI runs through the Nix path, so reproduce CI failures from inside the dev shell.

## Canonical recipes

Always invoke `just` rather than inventing ad-hoc `cargo` invocations.

| Recipe | Underlying command | Purpose |
|---|---|---|
| `just format` | `cargo fmt --all` | Format Rust sources |
| `just lint` | `cargo clippy --all-targets -- -D warnings` | Lint with clippy (warnings are errors) |
| `just test` | `cargo test` | Run the full test suite |
| `just lint-actions` | `actionlint && zizmor --offline .github/workflows .github/actions action.yaml` | Lint + security audit `.github/workflows/*.yaml` and composite actions |
| `just build` | `cargo build` | Debug build |
| `just build-release` | `cargo build --release --locked` | Release build |
| `just install` | `cargo install --path . --locked` | Install to `~/.cargo/bin` |
| `just bench` | `cargo bench` | Run criterion benches under `benches/` |
| `just coverage` | `cargo llvm-cov --workspace --lcov ... && cargo llvm-cov report` | Generate `lcov.info` and print a per-file summary |

### Pre-PR check

Run the full chain before declaring work complete — it mirrors the four CI jobs exactly:

```sh
nix develop -c just format lint test lint-actions
```

### Proto dev-loop

`browse`'s HTTP API is defined in `proto/ravelact/browse/v1/browse.proto` and consumed by both the Rust server and the React SPA via generated code committed under `src/cli/render/browse/{proto,connect}/` and `web/src/proto/`. After editing the `.proto` file, regenerate the vendored output:

```sh
nix develop -c just proto-gen        # regenerates Rust + TS from proto/
nix develop -c just proto-lint       # buf lint (also runs as part of `just lint`)
nix develop -c just proto-check-drift # CI mirror: regen + git diff --exit-code
```

`just proto-gen` invokes three plugins that ship as standalone `cargo install` binaries (not in nixpkgs):

```sh
nix develop -c cargo install --locked protoc-gen-buffa protoc-gen-buffa-packaging connectrpc-codegen
```

`protoc-gen-es` (the TypeScript generator) lives in `web/node_modules/.bin/` after `pnpm install`. `just proto-gen` precondition-checks all four binaries and prints the install command if any is missing.

#### Why `buffa`, not `prost`

The browse stack uses [`buffa`](https://github.com/anthropics/buffa) for proto messages and [`connectrpc`](https://github.com/anthropics/connect-rust) for the service runtime — both maintained by Anthropic and designed to compose. `buffa` is zero-copy (returns borrowed `*View<'a>` types alongside owned structs) and supports the proto3 JSON canonical mapping out of the box, which Connect-Web on the SPA side expects. `prost` would force a JSON-codec adapter layer on the server and would not share message types with the `connectrpc` server crate. Contributors familiar with `prost` will find the generated `buffa` types unfamiliar (note `MessageField<T>` instead of `Option<T>` for nested messages, and the `.as_option()` accessor); the rest of the API is similar.

### Release builds

```sh
nix develop -c just build-release         # cargo-built release binary in target/release/
```

> [!NOTE]
> `nix build .#default` is intentionally unavailable while the `browse`
> subcommand's web frontend is being modernised. `rust-embed` requires
> `web/dist/` at compile time, but the Nix crane sandbox cannot run
> `pnpm install && vite build`. A follow-up plan will restore the `nix
> build` path via `pkgs.pnpm.fetchDeps`. Use `just build-release` in the
> meantime.

### Browse subcommand: dev workflow

`ravelact browse` (the local GUI for the workflow graph) ships as a React +
Vite SPA bundled into the binary via `rust-embed`. Two iteration modes:

```sh
# Production / smoke-test path. Builds web/dist/ then the Rust binary.
nix develop -c just build-release
./target/release/ravelact --root . browse              # opens http://127.0.0.1:<port>/

# HMR dev loop (two terminals).
# Terminal 1: backend API on :7878
nix develop -c cargo run --release -- --root . browse --port 7878 --no-open
# Terminal 2: Vite dev server with /ravelact.browse.v1.BrowseService/* proxied to :7878
cd web && nix develop -c pnpm dev          # http://localhost:5173
```

The SPA talks to the backend via a generated ConnectRPC client at
`POST /ravelact.browse.v1.BrowseService/<Method>` (was: hand-written
`GET /api/<thing>` in 0.0.6 and earlier — see [#39](https://github.com/wadackel/ravelact/issues/39)).

Frontend-only checks:

```sh
nix develop -c pnpm --dir web test             # vitest unit tests
nix develop -c pnpm --dir web exec tsc --noEmit
nix develop -c pnpm --dir web e2e              # playwright (requires browsers installed once)
```

#### Default exclude: `tests/fixtures/**`

`browse` prepends `tests/fixtures/**` to the `--exclude` set so the dogfood
view stays focused on production workflows. Test-fixture local-actions
otherwise dominate the graph as orphan nodes (roughly three-quarters of
ravelact's own local-actions live under that path). Pass
`--include-test-fixtures` to opt out — for example when adopting `browse`
in a repository that places real workflows under `tests/fixtures/`. The
flag is intentionally browse-only; `impact`, `trace`, `orphans`, and the
other subcommands continue to honour the user-supplied `--exclude` set
without modification.

Hub nodes — actions that are referenced from most workflows — can leave
the highlight set close to the whole graph; that is an expected limit of
bidirectional reachability rather than a bug.

#### Interaction model

Selecting a node fades the unrelated subgraph and opens the detail panel.
Tapping empty graph space clears both: it closes the panel and removes
the dim state. Escape and the panel's `×` button do the same thing. The
`.faded` class has a single writer (a React effect keyed on the
selection), so the visual state always matches the panel state.

#### Performance check at 300-workflow scale

`script/perf-check-browse.ts` is a Deno harness that measures the browse
SPA at two scales — the host repo (~16 nodes) and a synthetic 300-workflow
estate it generates in a TempDir. It captures initial load time, drag FPS,
post-pan settle time, tap → highlight latency p50/p95 across 20 distinct
nodes, `/api/graph` payload bytes, and a coarse JS heap snapshot. Output
lands in `.wadackel/qa/<timestamp>_browse-perf-300/` (recording, screenshots,
`report.md`), which is git-ignored by the user's global rules.

Prerequisites:

- A current release binary at `./target/release/ravelact` — run
  `nix develop -c just build-release` first.
- An agent-browser state file at `~/.agent-browser-state/main.json` —
  refresh it with `ab-state-refresh` against a logged-in Chrome.
- Deno on `PATH`. Deno is **not** wired into the flake.nix dev shell yet;
  install it with `brew install deno`, or add it to `flake.nix` in a
  follow-up PR if you want `nix develop -c` to provide it.

Run it with:

```sh
deno run --allow-all script/perf-check-browse.ts
```

Baseline numbers from the May 2026 measurement (on the maintainer's
machine — absolute values are machine-relative, only the dogfood vs.
synthetic-300 delta is portable):

- initial load (timeOrigin → first ready): ~265 ms / ~465 ms
- drag FPS (texture path, `cy.panBy` 3 s sample): ~60 / ~60
- highlight latency p95 across 20 distinct nodes: ~1 ms / ~3 ms
- /api/graph payload: ~8.7 KB / ~100 KB
- coarse JS heap initial: ~7 MB / ~13 MB

All four guidance thresholds (load < 5 s, p95 highlight < 200 ms, drag
FPS ≥ 30, sublinear heap growth) cleared with substantial margin at the
300-workflow scale. Cytoscape and `cytoscape-dagre` versions are tracked
via `web/package.json`.

If you change `tests/e2e_browse.rs::write_synthetic_estate`, update the
mirrored `writeSyntheticEstate` in `script/perf-check-browse.ts` to keep
shape parity. The harness cross-checks parity each run by calling
`ravelact dump | jq` on the generated estate and asserting the expected
workflow / reusable counts.

#### README screenshots

`web/scripts/snapshot-readme.ts` regenerates the four PNGs committed
under `docs/images/browse-*.png` (referenced from the hero and `Browse`
section of `README.md`). Run it after any UI change that should be
reflected in the README:

```sh
nix develop -c just snapshot-readme            # verify only — fails on drift
nix develop -c just snapshot-readme --update   # rewrite committed PNGs
```

The script is Node + tsx + `@playwright/test`, drives Chromium against
`./target/release/ravelact` (so run `nix develop -c just build-release`
first), captures four shots at viewport `1440×900` / DPR 2 / locale
`en-US` / `prefers-reduced-motion: reduce`, and runs `oxipng -o 4
--strip safe` on each emitted PNG. The Playwright Chromium binary lives
in the global cache — install it once with `cd web && nix develop -c
pnpm exec playwright install chromium`.

Without `--update` the script exits non-zero when the regenerated PNGs
differ from the committed bytes, leaving `<name>.png.new` files behind
for inspection. Do not run concurrently with `pnpm e2e` — both can bind
localhost ports.

### GitHub Action installer

The repository root `action.yaml` is the public setup action used as `uses: wadackel/ravelact@...`. It installs a GitHub Release binary onto `PATH`, verifies its checksum, and fails if the installed binary cannot execute on the runner. It does not run analysis commands itself and does not fall back to `cargo install`. Keep the public input surface minimal. The initial supported input is `version`, which overrides the action ref when callers need to install a specific released binary.

When changing the action or `script/install-ravelact-action.sh`, run:

```sh
nix develop -c just lint-actions
bash -n script/install-ravelact-action.sh
RAVELACT_VERSION= RAVELACT_ACTION_REF=v0.0.5 RUNNER_OS=Linux RUNNER_ARCH=X64 script/install-ravelact-action.sh --resolve-only
```

The CI smoke job builds a local release-shaped fixture, serves it with a job-local HTTP server, and points the action at that fixture with the internal `RAVELACT_RELEASE_BASE_URL` environment variable. This keeps the installer path deterministic without relying on an already-published release. The action itself is only available from the first release tag that contains `action.yaml`; older tags can still be installed through `with.version` from a newer action ref or by manual binary download. A non-`v*` action ref must set `version` explicitly; use `version: latest` only when intentionally opting in to the latest GitHub Release.

GitHub Release assets are native `cargo build --release --locked` binaries built on each release runner. Do not publish `nix build .#default` outputs as GitHub Release assets; Nix-built outputs are for the Nix package path and can embed Nix store runtime paths that are not portable to regular Ubuntu runners.

### crates.io publishing

Crate publishing runs from `.github/workflows/release.yaml` on `v*` tag pushes only. Manual `workflow_dispatch` runs build the release artifacts but do not publish to crates.io.

The `publish-crate` job uses the GitHub environment named `crates-io` and crates.io trusted publishing. Maintainers must configure the crate's trusted publisher entry for this repository, workflow, and environment. The workflow authenticates with `rust-lang/crates-io-auth-action`, verifies that the tag name matches `v<package.version>`, and skips `cargo publish` when the exact crate version already exists on crates.io. If the `crates-io` environment has required reviewers, reruns still require that approval before the preflight can determine that a version is already published.

`release-plz` remains responsible for release PRs and tags, but it must not publish crates or create GitHub Releases directly. Keep `release-plz.toml` set to `publish = false`, `git_only = true`, and `git_release_enable = false`; crate publishing and GitHub Release creation belong to the tag release workflow so artifacts can be attached before immutable releases are published.

For the first crates.io release after this workflow lands, create a new release tag rather than rerunning an old tag. Old tags point at older commits and will not contain the current publishing workflow changes. If an immutable GitHub Release already exists for a tag, create a new tag or resolve the existing release manually; the workflow intentionally refuses to attach assets after publication.

Re-read the current [crates.io trusted publishing documentation](https://crates.io/docs/trusted-publishing) before changing the publish job; the workflow depends on OIDC permissions such as `id-token: write` and a SHA-pinned authentication action in `release.yaml`.

## Snapshot tests

Integration tests under `tests/` (notably `tests/e2e_oss.rs`) use [`insta`](https://insta.rs/) snapshots stored in `tests/snapshots/`. When IR shape or output formatting changes, expect snapshot diffs.

- **Review interactively**:

  ```sh
  nix develop -c cargo insta review
  ```

- **Bulk update** (use sparingly, only after eyeballing the diff):

  ```sh
  INSTA_UPDATE=always cargo test
  ```

> [!WARNING]
> Never blindly overwrite `.snap` files by hand. Always review the diff first to confirm the change is intentional — silent snapshot churn is the #1 source of behavioral regressions slipping through review.

## Findings fixtures

The SARIF finding-overlay tests (`src/findings/`, `tests/findings*.rs`) are driven by synthetic estates under `tests/fixtures/synthetic/`:

- `zizmor-findings/` — intentionally-vulnerable workflows plus a committed `zizmor.sarif` (and an `actionlint.sarif` for multi-source overlay coverage).
- `actionlint-findings/` — intentionally-broken workflows plus a committed `actionlint.sarif`.

The `.sarif` files are **regenerated, not hand-written**, and normalized for deterministic diffs. Each directory's `README.md` documents the exact regeneration command — re-run it after a zizmor / actionlint upgrade or a fixture edit. These fixtures live under `tests/fixtures/`, which `just lint-actions` does not scan, so their deliberate problems never trip the repo's own CI.

## IR cache

Build artifacts are cached at `${XDG_STATE_HOME}/ravelact/repo-<sha8>/cache.json` (or `$HOME/.local/state/ravelact/...` when `XDG_STATE_HOME` is unset). The cache lives **outside** the repository so adopters do not need any `.gitignore` entry. The cache is keyed by `SCHEMA_VERSION` in the IR. See README "Cache location" for the full prerequisite (`XDG_STATE_HOME` or `HOME` required).

- **Adding fields to IR types**: new fields MUST use `#[serde(default)]` so older cache files still load without forcing a full rebuild.
- **Forcing a rebuild**: pass `--no-cache` to any IR-consuming command (or remove the per-repo subdirectory under `${XDG_STATE_HOME:-$HOME/.local/state}/ravelact/`).
- **Confusing behavior during development**: a stale cache is the most likely cause. Try `--no-cache` first.

## Coverage

Code coverage is measured with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov), bundled in the dev shell.

### Coverage policy

Every file under `src/` must report **≥ 90% line coverage**. The `coverage` CI job fails when any file drops below the floor, so a PR that regresses coverage cannot merge. The threshold is hard-coded in `.github/workflows/ci.yaml` (search for `THRESHOLD=90`); changing it requires editing the workflow.

- The floor applies **per file**, not to the workspace total. A 95% total can still fail if one file is at 87%.
- The metric is line coverage from `cargo llvm-cov --lcov`. Branch / region / function coverage are not gated.
- The floor applies to `src/` only. `tests/`, `benches/`, `web/`, and generated code are out of scope.

### Local run

```sh
nix develop -c just coverage         # generate lcov.info + print summary
nix develop -c just coverage-gate    # generate lcov.info + fail if any src/ file < 90%
```

Outputs:

- `lcov.info` at the repo root (gitignored) for editor integrations such as VS Code's *Coverage Gutters*.
- A per-file summary printed to the terminal.
- `coverage-gate` exits non-zero with `<file>: XX.XX% below 90%` lines for each regression, matching what the CI gate prints.

### PR comment

The `coverage` job in `ci.yaml` posts a sticky comment on every PR that shows:

- **Total coverage** with delta vs `main` in percentage points.
- **Per-file coverage** with hit/found line counts and per-file delta vs `main`. Files added in the PR appear with `Δ = new`; if no base lcov is available (first run, retention expiry), all deltas show `N/A`.

The base lcov is fetched from the most recent successful CI run on `main`, so the diff lags `main` by one CI run. The comment is updated in place via a marker (`<!-- ravelact-coverage-report:v1 -->`). The comment posts **before** the gate step runs, so the table is still visible even on a failed coverage run.

### Excluding code that genuinely cannot be unit-tested

`#[coverage(off)]` is gated behind an unstable feature and `rust-toolchain.toml` pins stable, so it is not available today. When a piece of code is genuinely untestable (e.g. a TCP bind, OS signal handler, `webbrowser::open` call), the supported workflow is:

1. Extract the untestable code into the smallest possible module file (e.g. `src/cli/render/foo_runtime.rs`).
2. Add `--ignore-filename-regex 'src/cli/render/foo_runtime\.rs'` to the `coverage` recipe in `justfile`.
3. Call out in the PR description **which** lines were excluded and **why** they cannot be tested.

This PR introduces no exclusions; `tests/e2e_browse.rs` covers the runtime tail of `cli/render/browse/mod.rs` (TCP bind, browser launch, signal handler) as an integration smoke test rather than a unit-coverage source. Note that `cargo-llvm-cov` does **not** automatically merge coverage from the spawned `ravelact` subprocess into the main `lcov.info`, so subprocess-only code paths still count as uncovered. Cover them with in-file unit tests against extracted helpers, not by spawning the binary.

### Known false-negative areas

- **CLI subprocess paths**: any code only reached by spawning the `ravelact` binary (e.g. `tests/e2e_browse.rs` and `tests/completions.rs`) does not contribute to `lcov.info`. Cover those branches via in-file unit tests (see `src/cli/render/browse/mod.rs::mod tests` for the pattern) rather than relying on the integration tests.
- **Error formatting helpers** that print to stderr only fire on rare malformed inputs and are intentionally not exhaustively tested.

## Workflow file conventions

- Project workflow files use the `.yaml` extension, not `.yml`. This was standardized in [#38](https://github.com/wadackel/ravelact/pull/38) — match it when adding new workflows.
- Pin every GitHub Actions step to a full commit SHA, never a version tag. Both `ci.yaml` and `release.yaml` follow this convention; new workflow steps must too.

## Commits and PRs

- **Conventional Commits with a scope** — examples from history: `feat(trace): …`, `fix(parser): …`, `chore: …`, `build(nix): …`, `refactor(walk): …`. Common scopes: `trace`, `parser`, `query`, `cache`, `nix`, `ci`, `walk`, `ir`.
- PR titles and bodies are squash-merged with `(#NN)` appended on merge — write the title as if it were the final commit message.
- All committed artifacts (code, comments, commit messages, PR titles/bodies, docs) are written in **English**.

## Plain `cargo` fallback

`cargo build` / `cargo test` work outside the Nix dev shell, but CI uses the Nix path. If a failure reproduces on plain `cargo` but not in `nix develop`, suspect a toolchain mismatch and reproduce inside the dev shell before filing it as a project bug.

## See also

- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — contributor on-ramp.
