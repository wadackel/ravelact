# `orphans`

Report declared-but-unused items across the estate. Emits four kinds in one
pass:

1. Reusable workflows that nothing references.
2. Local actions (composite / JavaScript / Docker) that nothing references.
3. Declared `inputs:` that the callee body never references.
4. Declared `outputs:` that no caller consumes via
   `needs.<job>.outputs.<X>` / `steps.<id>.outputs.<X>`.

Always exits 0 — the report is informational, not a CI gate.

```sh
ravelact orphans
ravelact orphans --format json | jq '.workflows'
```
