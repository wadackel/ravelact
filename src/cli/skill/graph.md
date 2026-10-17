# `graph`

Render the call graph as a Mermaid `graph LR`. Entry workflows are grouped
into `subgraph` blocks per trigger event (multi-trigger workflows appear as
one alias per trigger); reusable workflows / local actions / external actions
are shared nodes; edges follow `uses` / `workflow_call` / `workflow_run`.

- `--event <event>` — restrict to entry workflows for the given trigger
  (recommended for large estates where the unfiltered graph is too dense to
  read).
- `--format text` (default) — raw Mermaid suitable for `> graph.mmd`.
- `--format markdown` — wraps the same Mermaid in a `### Graph` heading + a
  fenced ` ```mermaid ` block, ready to paste into a PR comment or GitHub Job
  Summary.
- `--findings <PATH> --highlight findings` — annotate node labels with
  finding-count badges and color nodes by severity; `--show-priority` bases the
  counts on graph priority (see `ravelact skill findings`).
- Use `dump` for IR JSON.

```sh
ravelact graph --event push > push.mmd
ravelact graph --event pull_request --format markdown
```
