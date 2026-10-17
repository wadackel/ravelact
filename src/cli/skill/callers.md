# `callers <target>...`

List every call site that references the given workflow or local-action target,
relative to the repository root. Reads from stdin (one path per line) when no
positional targets are given and stdin is piped (rg/grep convention).

```sh
ravelact callers .github/workflows/_build.yaml
ravelact callers .github/actions/setup
echo .github/workflows/_build.yaml | ravelact callers
```

`text` output renders one caller per line as `file:job:index` with a
`# <target>` header per input. `json` emits an array of `{target, hits}`
objects, preserving input order. `markdown` emits a `### Callers` heading plus
a table of targets and call sites.
