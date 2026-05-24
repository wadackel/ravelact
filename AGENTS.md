# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.

## Project

`ravelact` is a Rust CLI for static analysis of GitHub Actions workflow estates. It builds an IR from `.github/workflows/*.yaml` and `action.yaml` files and runs forward / reverse / orphan / dump / impact / lint / check / suggest queries against it.

## Canonical workflow

All commands run inside the Nix dev shell. Use `just` recipes — do not invent ad-hoc cargo invocations.

- `nix develop -c just format lint test lint-actions` — full pre-PR check; mirrors GitHub Actions exactly (the four CI jobs). Run this before declaring work complete.
- `nix develop -c just test` — `cargo test`
- `nix develop -c just lint` — `cargo clippy --all-targets -- -D warnings` (warnings are errors)
- `nix develop -c just format` — `cargo fmt --all`
- `nix develop -c just build-release` — `cargo build --release --locked`
- `nix build .#default` — Nix-built release binary at `result/bin/ravelact`

Plain `cargo build` / `cargo test` work too, but CI uses the Nix path; reproduce failures there.

## Snapshot tests

Integration tests under `tests/` (notably `tests/e2e_oss.rs`) use `insta` snapshots stored in `tests/snapshots/`. When IR shape or output formatting changes, expect snapshot diffs.

- Update snapshots: `nix develop -c cargo insta review` (interactive accept/reject) or `INSTA_UPDATE=always cargo test`.
- Never blindly overwrite `.snap` files by hand — review the diff first to confirm the change is intentional.

## IR cache

Build artifacts are cached at `${XDG_STATE_HOME}/ravelact/repo-<sha8>/cache.json` (or `$HOME/.local/state/ravelact/...` when `XDG_STATE_HOME` is unset). The cache lives **outside** the repository so this project — and any adopter — does not need any `.gitignore` entry. The cache is keyed by `SCHEMA_VERSION` in the IR; new fields added to IR types must use `#[serde(default)]` so older caches still load. When a cache mismatch causes confusing behavior during development, pass `--no-cache` or remove the per-repo subdirectory under `${XDG_STATE_HOME:-$HOME/.local/state}/ravelact/`.

## Workflow files

Project workflow files use the `.yaml` extension (not `.yml`) — this was standardized in #38. Match that when adding new workflows.

## Frontend tooling

The pnpm version is managed solely by `flake.nix` (`pnpm_11`). `web/package.json` intentionally omits the `packageManager` field — Nix is the single source of truth for both Node and pnpm, and CI invokes pnpm exclusively via `nix develop --command pnpm ...`. Do not re-add `packageManager` or rely on Corepack. pnpm settings (exact-pin save prefix, minimum release age, allowed build scripts) live in `web/pnpm-workspace.yaml` because pnpm 10+ no longer reads them from `.npmrc`.

## GitHub Actions security

Pin every action to a full commit SHA, not a version tag. Both `ci.yaml` and `release.yaml` follow this; new workflow steps must too.

## GitHub Actions reference docs

When designing or implementing logic that interprets GitHub Actions semantics, you MUST invoke `/gh-actions-docs` first to fetch authoritative reference. The IR (`src/ir/`), parser (`src/parser/`), and queries (`src/query/`) all depend on accurate spec interpretation; do not rely on memorized GA semantics. If the relevant URL is missing from the catalog, search docs.github.com/en/actions and add it via PR.

## Commits and PRs

- Use Conventional Commits with a scope: `feat(trace): …`, `fix(parser): …`, `chore: …`, `build(nix): …`, `refactor(walk): …`. Common scopes seen in history: `trace`, `parser`, `query`, `cache`, `nix`, `ci`, `walk`, `ir`.
- PR titles and bodies are squash-merged with `(#NN)` appended on merge.

## Language

- All committed artifacts — code, comments, commit messages, PR titles, PR bodies, issue text, and code-adjacent docs — are written in **English**.
- Conversational replies to the user follow the user's own language setting (i.e., Japanese in this environment per the global config).
