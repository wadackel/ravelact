# `permissions`

Compute the effective `permissions:` scope across caller→callee chains and
surface:

1. Overly-broad coarse declarations (e.g. `permissions: write-all`) on entry
   workflows.
2. Callee declarations that exceed the caller (violates the GitHub Actions
   "monotonic decrease" rule).
3. Entry workflows with no permissions declared at any layer.

```sh
ravelact permissions
```
