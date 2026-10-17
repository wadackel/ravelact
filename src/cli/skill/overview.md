# ravelact

Static analysis CLI for GitHub Actions workflow estates. Builds an in-memory IR
from `.github/workflows/*.{yml,yaml}` and any `action.{yml,yaml}` in the
repository, then answers forward/reverse/orphan/dump/impact/wiring/check/suggest
queries against it.

The tool runs **offline** against the local repository — no GitHub API calls,
no token required. The workflow and action files being analysed are never
modified. The only file `ravelact` writes is its own IR cache at
`${XDG_STATE_HOME}/ravelact/repo-<sha8>/cache.json` (or
`$HOME/.local/state/ravelact/...` when `XDG_STATE_HOME` is unset). The cache
lives **outside** the repository, so no `.gitignore` entry is required. The
cache is created by `build` and refreshed automatically by other commands.

## When to use this skill

Trigger this skill when the user is working with GitHub Actions workflows and
asks about any of the following:

- "What runs when this PR is opened / on push?" → `ravelact skill trace`.
- "Which trigger events exist in this repo?" → `ravelact skill triggers`.
- "Who calls this reusable workflow / local action?" → `ravelact skill callers`.
- "If I change file X, which entry-point workflows are affected?" → `ravelact skill impact`.
- "Are there unused workflows / actions / inputs / outputs?" → `ravelact skill orphans`.
- "Are permissions / secrets propagated correctly across reusable-workflow chains?"
  → `ravelact skill permissions` or `ravelact skill secrets`.
- "Show me duplicated step sequences I could extract into a composite action."
  → `ravelact skill extract`.
- "Are these workflows near-duplicates of each other?" → `ravelact skill dedup`.
- "Render the call graph as a diagram." → `ravelact skill graph` (Mermaid).
- "Give me the IR as JSON for a custom query." → `ravelact skill dump`.
- Any consistency question about the workflow estate's wiring → `ravelact skill wiring`.
- "Where do the zizmor / actionlint findings sit in the call graph?" → overlay a
  SARIF report with `--findings` (see `ravelact skill findings`).

## Setup

Install the `ravelact` binary from a release or local build, then invoke it as
`ravelact <subcommand>`. The first analysis command in
any session should be `build` to populate the IR cache under
`${XDG_STATE_HOME:-$HOME/.local/state}/ravelact/` (subsequent commands reuse
the cache automatically).
Run `ravelact skill <topic>` for a command's flags, examples, and output before
invoking it.

```sh
ravelact build --root .
```

## Operating conventions

- If the user asks for an answer about the current repository, run the relevant
  `ravelact` command when the binary is available. If they ask for a CI snippet,
  review comment, dry-run, or plan, return the command sequence without running
  it.
- Prefer `--root .` in reusable snippets and CI examples. Omit it only when the
  current working directory is clearly the repository root.
- Start executable workflows with `ravelact build --root .` to populate the
  cache. Use `--no-cache` only when results may be stale after edits, not as the
  default substitute for `build`.
- For `impact`, guard empty diffs before invoking the command. Empty stdin is a
  usage error; it is not a clean "no impacted targets" result.
- `impact` reports downstream consumers of changed workflows/actions. It does
  not list the changed input nodes themselves; if the user wants those too,
  report them separately from the `impact` result.
- In CI, keep `permissions`, `secrets`, and `wiring` as the merge-gating checks.
  Put them in separate steps when the user wants clearer failure attribution.

## Global flags (apply to every subcommand)

- `--root <path>` — root directory to scan (default: current directory).
- `--no-cache` — skip the IR cache (under
  `${XDG_STATE_HOME:-$HOME/.local/state}/ravelact/`) for this invocation;
  force a full rebuild. Use after manual edits to workflow files when the
  cache may be stale.
- `--exclude <glob>` — exclude local-action manifests matching the glob
  (repeatable). Workflow files under `.github/workflows/` are not affected.
  Useful for skipping `tests/fixtures/**`-style intentional test data.
- `--color auto|always|never` — control ANSI color globally. JSON output never
  contains ANSI escapes regardless of this flag.

## Output formats

`--format` is opt-in per command. Most report commands accept
`text|json|markdown` (default `text`); `trace` accepts
`tree|table|json|markdown` (default `tree`); `graph` accepts `text|markdown`.
`dump` is always JSON, and `build` / `completion` produce no formatted output.

| Command       | Formats                              | Notes                                 |
|---------------|--------------------------------------|---------------------------------------|
| `trace`       | `tree`, `table`, `json`, `markdown`  | `--ascii` affects `tree` only         |
| `triggers`    | `text`, `json`, `markdown`           |                                       |
| `callers`     | `text`, `json`, `markdown`           |                                       |
| `impact`      | `text`, `json`, `markdown`           |                                       |
| `orphans`     | `text`, `json`, `markdown`           |                                       |
| `wiring`      | `text`, `json`, `markdown`           | exits 1 when findings exist           |
| `permissions` | `text`, `json`, `markdown`           | exits 1 when findings exist           |
| `secrets`     | `text`, `json`, `markdown`           | exits 1 when findings exist           |
| `extract`     | `text`, `json`, `markdown`           |                                       |
| `dedup`       | `text`, `json`, `markdown`           |                                       |
| `dump`        | fixed JSON                           | no `--format` flag                    |
| `graph`       | `text`, `markdown`                   | use `dump` for IR JSON                |
| `build`       | fixed text                           | no `--format` flag                    |
| `completion`  | shell snippet                        | no `--format` flag                    |
| `skill`       | fixed markdown                       | no `--format` flag                    |

`text` is human-readable; `json` is suitable for `jq`; `markdown` emits a
self-contained `### <Section>` heading + table (or a per-command "No … found"
message when empty — for example `impact` writes `No impacted targets found.`,
`orphans` writes `No unused declarations found.`, `extract` writes
`No extraction candidates found.`, `dedup` writes
`No near-duplicate workflow clusters found at threshold X.`) suitable for
inline embedding in PR comments and GitHub Job Summaries.

## Exit codes by command group

The CLI groups commands by exit-code policy. The grouping matters when wiring
commands into CI: the **Check** group is intended for merge gating; the others
are informational.

- **Inspect (exit 0; non-blocking reports)**: `trace`, `triggers`, `callers`, `impact`, `orphans`.
- **Check (exit 0/1; non-zero on findings)**: `permissions`, `secrets`, `wiring`.
  Use these in CI as merge-gating steps.
- **Suggest (exit 0; refactor candidates, non-mutating)**: `extract`, `dedup`.
- **Export (output artifacts)**: `dump`, `graph`.
- **Other**: `build` (cache management), `completion` (shell setup snippet), `skill` (this guidance).

`build` exits 0 on success. Like every command, it propagates a non-zero exit
on hard errors (unreadable files, malformed YAML), but absent such errors it
is informational and never gates CI.

## Output interpretation tips

- **JSON output is the contract**: when in doubt about field semantics, run
  `--format json` and inspect the keys directly. Text output is optimized for
  humans; JSON is optimized for downstream tooling.
- **Empty results are valid**: `orphans` with zero findings, `dedup` with no
  clusters, `impact` with no consumers — all exit 0 (or, for the **Check**
  group, exit 0 only when truly clean).
- **Cache staleness**: if a result looks wrong after editing a workflow file,
  re-run with `--no-cache` to force a rebuild before debugging deeper.

## Out of scope

`ravelact` does not:

- Execute workflows (use `act` for local execution).
- Detect security rule violations such as untrusted-input sinks (use `zizmor`
  or `octoscan`).
- Rewrite workflow files or perform automatic refactors (use `OpenRewrite` or
  `Actionforge`).
- Collect dynamic analysis (execution logs, CI metrics).
