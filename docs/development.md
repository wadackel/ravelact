# Development Guide

Reference for building, testing, and maintaining `ravelact` locally.

## Environment

The project ships a [Nix](https://nixos.org/) flake and a [`justfile`](https://just.systems/) for a reproducible toolchain. All canonical recipes run inside the Nix dev shell.

```sh
nix develop                  # enter dev shell (rust toolchain + just + jq + actionlint)
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
| `just lint-actions` | `actionlint` | Lint `.github/workflows/*.yaml` |
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

### Release builds

```sh
nix develop -c just build-release         # cargo-built release binary in target/release/
nix build .#default                       # Nix-built release binary at result/bin/ravelact
```

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

## IR cache

Build artifacts are cached at `${XDG_STATE_HOME}/ravelact/repo-<sha8>/cache.json` (or `$HOME/.local/state/ravelact/...` when `XDG_STATE_HOME` is unset). The cache lives **outside** the repository so adopters do not need any `.gitignore` entry. The cache is keyed by `SCHEMA_VERSION` in the IR. See README "Cache location" for the full prerequisite (`XDG_STATE_HOME` or `HOME` required).

- **Adding fields to IR types**: new fields MUST use `#[serde(default)]` so older cache files still load without forcing a full rebuild.
- **Forcing a rebuild**: pass `--no-cache` to any IR-consuming command (or remove the per-repo subdirectory under `${XDG_STATE_HOME:-$HOME/.local/state}/ravelact/`).
- **Confusing behavior during development**: a stale cache is the most likely cause. Try `--no-cache` first.

## Coverage

Code coverage is measured with [`cargo-llvm-cov`](https://github.com/taiki-e/cargo-llvm-cov), bundled in the dev shell. Coverage is **informational only** — there is no threshold and the CI job does not block PR merge.

### Local run

```sh
nix develop -c just coverage
```

Outputs:

- `lcov.info` at the repo root (gitignored) for editor integrations such as VS Code's *Coverage Gutters*.
- A per-file summary printed to the terminal.

### PR comment

The `coverage` job in `ci.yaml` posts a sticky comment on every PR that shows:

- **Total coverage** with delta vs `main` in percentage points.
- **Per-file coverage** with hit/found line counts and per-file delta vs `main`. Files added in the PR appear with `Δ = new`; if no base lcov is available (first run, retention expiry), all deltas show `N/A`.

The base lcov is fetched from the most recent successful CI run on `main`, so the diff lags `main` by one CI run. The comment is updated in place via a marker (`<!-- ravelact-coverage-report:v1 -->`).

### Known false-negative areas

- **CLI argument parsing** in `src/cli/` is exercised end-to-end via `assert_cmd` in `tests/`, but `cargo-llvm-cov` only counts coverage from the test binaries themselves; argument-parsing branches reached only by the spawned `ravelact` subprocess can show as uncovered even when tests do drive them. Treat low coverage on `cli/parse*.rs` as informational.
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
