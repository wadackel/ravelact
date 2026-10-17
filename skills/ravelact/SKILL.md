---
name: ravelact
description: >-
  Run `ravelact` to perform static analysis on a
  GitHub Actions workflow estate — forward/reverse graph queries (trace, callers),
  trigger discovery, PR-diff impact, orphan detection, permissions/secrets propagation auditing across
  reusable-workflow chains, refactor helpers (composite extraction, near-duplicate
  clusters), and IR export (JSON, Mermaid). Use when working with
  `.github/workflows/*.{yml,yaml}` and `action.{yml,yaml}` files — for PR impact analysis,
  dependency-graph questions ("who calls this reusable workflow?", "what runs on
  push?"), permissions auditing, or refactoring a large GitHub Actions estate.
  Requires the `ravelact` binary on PATH first.
license: MIT
compatibility: Requires the ravelact binary on PATH.
metadata:
  repository: wadackel/ravelact
---
# ravelact

The usage guide for `ravelact` ships inside the binary, so it always matches
the installed version. Load it before running any other `ravelact` command:

```sh
ravelact skill
```

Follow the printed guide. It covers which command answers which question,
output formats, exit codes, and common CI recipes.

If `ravelact` is not on PATH, ask the user to install it from
https://github.com/wadackel/ravelact#installation. If the command fails with
`unrecognized subcommand 'skill'`, the installed binary predates the embedded
guide. Ask the user to upgrade, or fall back to `ravelact --help` and
`ravelact <command> --help`.
