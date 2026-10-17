# `impact`

Reverse impact analysis. Given a list of changed files (workflow YAML, action
manifest, or any file under a local action directory), list the entry-point
workflows and local-action consumers transitively affected. The input nodes
themselves are excluded; only **downstream consumers** appear.

The typical CI usage pipes `git diff --name-only` into the command:

```sh
git diff --name-only origin/main... | ravelact impact
git diff --name-only origin/main... | ravelact impact --format markdown
```

Errors on empty stdin (no input is treated as a usage error, not a zero-finding
result) — handle the empty-diff case in calling scripts.
