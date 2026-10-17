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
compatibility: Requires a ravelact binary that provides `ravelact skill` on PATH.
allowed-tools: Bash(ravelact skill:*)
metadata:
  repository: wadackel/ravelact
---

# ravelact

The guidance for this skill ships inside the `ravelact` binary, so it always
matches the installed version. Before running any other `ravelact` command,
read the overview first, then the topic for each command you plan to run:

```sh
ravelact skill            # 1. overview: conventions, exit codes, topic list
ravelact skill <topic>    # 2. one command (e.g. `callers`) or guide (`findings`, `recipes`)
```

If `ravelact` is not on PATH, or `skill` is an unrecognized subcommand, the
binary is missing or too old: tell the user to install or upgrade it
(https://github.com/wadackel/ravelact#installation) and stop.
