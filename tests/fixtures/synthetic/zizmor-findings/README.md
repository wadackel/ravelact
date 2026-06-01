# zizmor-findings fixture

Intentionally-vulnerable estate used to drive the finding-overlay normalization
pipeline (`src/findings/`). The workflows here trigger real zizmor audits so the
committed `zizmor.sarif` is close to real-world data.

`zizmor.sarif` is regenerated, not hand-written. It is **normalized** for
deterministic diffs: `tool.driver.version` / `semanticVersion` are stripped and
`runs[0].results[]` are sorted by `(ruleId, uri, startLine)`.

## Regenerate (after a zizmor upgrade or fixture edit)

```bash
cd tests/fixtures/synthetic/zizmor-findings
NORMALIZE='del(.runs[0].tool.driver.version, .runs[0].tool.driver.semanticVersion) | .runs[0].results |= sort_by(.ruleId, .locations[0].physicalLocation.artifactLocation.uri, .locations[0].physicalLocation.region.startLine)'
nix develop ../../../.. -c zizmor --format sarif --offline . | jq "$NORMALIZE" > zizmor.sarif
```

Notes:
- Run zizmor with `.` as the target from this directory so result URIs are
  root-relative (`.github/workflows/ci.yml`), matching ravelact IR node IDs.
- This fixture lives under `tests/fixtures/`, which `just lint-actions` does NOT
  scan, so its deliberate vulnerabilities never trip the repo's own zizmor CI.
- zizmor 1.25.2 emits no `security-severity` / `rank`; per-result severity lives
  in `properties["zizmor/severity"]` (High/Medium/Low) plus the coarse `level`.

## actionlint.sarif (multi-source overlay)

This estate also carries `actionlint.sarif` so the overlay can be exercised with
two sources at once. actionlint flags the same untrusted-input steps zizmor does
(`ci.yml:14`, `pr-target.yml:15`), demonstrating both sources on one node.
Regenerate it with the shared template (`../../actionlint-sarif.tmpl`):

```bash
cd tests/fixtures/synthetic/zizmor-findings
NORMALIZE='.runs[0].results |= sort_by(.ruleId, .locations[0].physicalLocation.artifactLocation.uri, .locations[0].physicalLocation.region.startLine, .locations[0].physicalLocation.region.startColumn)'
nix develop ../../../.. -c bash -c \
  "actionlint -shellcheck= -pyflakes= -format \"\$(cat ../../actionlint-sarif.tmpl)\" .github/workflows/*.yml | jq '$NORMALIZE'" \
  > actionlint.sarif
```

(The estate keeps the legacy `.yml` extension, so these URIs are `.yml`. See
`../actionlint-findings/README.md` for the dedicated actionlint fixture.)
