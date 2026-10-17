# `trace <event>`

Forward walk from a trigger event (e.g. `push`, `pull_request`,
`workflow_dispatch`, `schedule`, `workflow_run`). Surfaces every workflow that
fires for the event and the chain of `workflow_call` / `uses` / `workflow_run`
edges reachable from each entry point.

Filtering flags (all repeatable; OR within one flag, AND across flags):

- `--type <activity>` — restrict to entry-points whose `types:` declaration
  includes the activity (e.g. `--type opened` for `pull_request`).
  `pull_request` workflows that omit `types:` still match the GitHub default
  subset (`opened` / `synchronize` / `reopened`).
- `--branch <name>` / `--tag <name>` / `--path <pattern>` — test against the
  trigger's `branches:` / `tags:` / `paths:` filter fields. Each `--path X` is
  interpreted as the single-file changeset of `X`. For `push`, a trigger that
  defines only branch filters does not match `--tag` (and the reverse), so
  `--branch` with `--tag` keeps only triggers that accept both; `--tag`
  without `--branch` skips `paths:` filters; `--path` without `--branch` or
  `--tag` is treated as a branch push.

Rendering:

- `--format tree` (default) — Unicode box-drawing tree.
- `--format table` — 5-column audit table (`DEP / KIND / EDGE / TARGET / NOTE`)
  suitable for grep and CI logs.
- `--format json` — structured trace entries for automation.
- `--format markdown` — `### Trace` heading plus the trace rows as a Markdown table.
- `--ascii` — fall back to ASCII border characters in the tree view.
- `--findings <PATH>` / `--show-findings` / `--show-priority` — overlay external
  SARIF findings scoped to the reachable nodes (see `ravelact skill findings`).

```sh
ravelact trace push
ravelact trace pull_request --type opened --branch main
ravelact trace push --format table
```
