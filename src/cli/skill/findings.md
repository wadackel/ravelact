# Findings overlay

Overlay external **SARIF** findings onto the call graph, so a finding is read in
graph context. Format is auto-detected (`$schema` mentioning `sarif`, or a
top-level `runs` array); `zizmor` / `actionlint` are first-class, other SARIF
sources pass through. Informational only — never changes exit codes, so keep the
source tool as its own gate. On `trace` / `callers` / `impact` / `orphans`:
`--findings <PATH>` (repeatable, concatenated, no dedup; scoped to the nodes that
command reports), `--show-findings` (co-display in `text` / `markdown`; `json`
always includes findings when `--findings` is set), `--show-priority` (ravelact
**graph priority** + reasons beside the tool **source severity**: promoted when
reachable from `pull_request_target` / `workflow_run` or write-capable
(`write-all` / `contents` / `id-token: write`), demoted when orphaned / test
fixtures). `graph` uses `--highlight findings`; `browse` uses `--findings`
(badges, colors, Findings tab, context filters, dangerous-path edges).
