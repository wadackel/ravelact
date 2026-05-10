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
