# `wiring`

Verify that declared dependency edges resolve and that observable dispatches
are declared. Reports four finding kinds:

- `unannotated-dispatch` — `gh workflow run X` literals in `run:` blocks
  that are not paired with a matching `# ravelact:dispatches X` annotation.
- `dangling-annotation` — `# ravelact:` comments whose target no longer
  resolves to a local workflow.
- `dangling-workflow-run` — `on.workflow_run.workflows: [Name]` entries
  that cannot be resolved to any local workflow by display name or path.
- `dangling-local-uses` — `uses: ./<path>` references (step-level local
  actions or job-level local reusable workflows) whose target is not
  present in the IR (catches typos and stale paths).

```sh
ravelact wiring
```
