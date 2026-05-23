# Contributing to ravelact

Thanks for your interest in `ravelact`. This page is the short on-ramp; the full development reference lives in [`docs/development.md`](./docs/development.md).

## Quick start

Clone the repo and drop into the Nix dev shell — that gives you the pinned Rust toolchain, `just`, `actionlint`, and `zizmor` in one go:

```sh
git clone https://github.com/wadackel/ravelact.git
cd ravelact
nix develop
```

From there, every recipe (`just format`, `just lint`, `just test`, the pre-PR chain, release builds) is documented in [`docs/development.md`](./docs/development.md). No commands are duplicated here on purpose — single source of truth.

## Submitting changes

- **Branch off `main`** and open a PR. PRs are squash-merged; the PR title becomes the final commit message with `(#NN)` appended.
- **Conventional Commits with a scope** — examples: `feat(trace): …`, `fix(parser): …`, `chore: …`, `build(nix): …`. Common scopes: `trace`, `parser`, `query`, `cache`, `nix`, `ci`, `walk`, `ir`.
- **Run the pre-PR check** before requesting review (the recipe is in [`docs/development.md`](./docs/development.md#pre-pr-check)) — CI mirrors it exactly, so a green local run is a good predictor.
- **Write in English** — all committed artifacts (code, comments, commit messages, PR title/body, docs) are English. Conversational replies in issues / PR comments may follow your own language.
- **Pin GitHub Actions to full commit SHAs**, never a version tag. Existing workflows follow this; new ones must too.

## Snapshot tests

Integration tests under `tests/` use [`insta`](https://insta.rs/) snapshots. If your change shifts IR shape or output formatting, expect snapshot diffs.

> [!WARNING]
> Never blindly overwrite `.snap` files. Review every diff first via `cargo insta review` (in the dev shell) and confirm the change is intentional. Silent snapshot churn is the easiest way to land an unintended regression.

## Reporting issues

Please include the command you ran, the actual output, the expected output, and (if relevant) a minimal `.github/workflows/` fixture that reproduces the behavior. Issues without repro steps are hard to action.
