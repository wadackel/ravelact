# `secrets`

Trace `secrets:` propagation across entry-point → reusable-workflow chains and
surface:

1. `MissingSecretPropagation` — callee declares a `secrets:` requirement the
   caller does not pass.
2. `SecretsInheritChainBreak` — `secrets: inherit` is broken mid-chain so the
   leaf cannot receive caller secrets.
3. `EnvironmentInWorkflowCallCallee` — environment-scoped secrets used in a
   reusable callee that does not declare the environment.

External (cross-repo) callees are opaque and skipped.

```sh
ravelact secrets
```
