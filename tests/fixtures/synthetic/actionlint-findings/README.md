# actionlint-findings fixture

Intentionally-broken estate used to drive the finding-overlay normalization
pipeline (`src/findings/`) with **actionlint** as the source tool. The workflows
here trigger a representative spread of actionlint rule `kind`s at file / job /
step locations so the committed `actionlint.sarif` is close to real-world data.

actionlint emits no native SARIF, so it is driven through a Go `-format` template
shared with the other actionlint fixture: `../../actionlint-sarif.tmpl`. The
template carries no `tool.driver.rules` array and no version fields, so the
committed SARIF only depends on the rule messages themselves.

`actionlint.sarif` is regenerated, not hand-written. It is **normalized** for
deterministic diffs: `runs[0].results[]` are sorted by
`(ruleId, uri, startLine, startColumn)`.

## Regenerate (after an actionlint upgrade or fixture edit)

```bash
cd tests/fixtures/synthetic/actionlint-findings
NORMALIZE='.runs[0].results |= sort_by(.ruleId, .locations[0].physicalLocation.artifactLocation.uri, .locations[0].physicalLocation.region.startLine, .locations[0].physicalLocation.region.startColumn)'
nix develop ../../../.. -c bash -c \
  "actionlint -shellcheck= -pyflakes= -format \"\$(cat ../../actionlint-sarif.tmpl)\" .github/workflows/*.yaml | jq '$NORMALIZE'" \
  > actionlint.sarif
```

Notes:
- Run actionlint from inside this directory with the workflow files as explicit
  relative paths so result URIs are root-relative (`.github/workflows/ci.yaml`),
  matching ravelact IR node IDs. Workflow files use the `.yaml` extension (#38).
- `-shellcheck=` / `-pyflakes=` disable the external checkers so the SARIF is
  hermetic and independent of whether shellcheck/pyflakes are installed.
- This fixture lives under `tests/fixtures/`, which `just lint-actions` does NOT
  scan, so its deliberate mistakes never trip the repo's own actionlint CI.
- actionlint comes from `flake.lock`'s nixpkgs rev (no explicit version pin).
  Rule message text can change across actionlint releases, so a nixpkgs/actionlint
  bump may require regenerating this file. The crafted workflows deliberately
  avoid rules whose message embeds version-volatile data (e.g. `runner-label`,
  which lists every known runner label).
