# Common workflows

**PR impact comment** (markdown output piped into `gh pr comment`):

```sh
git diff --name-only origin/main... \
  | ravelact impact --format markdown \
  | gh pr comment --body-file -
```

**Estate-wide hygiene check** (combine multiple reports into one Job Summary):

```sh
{
  ravelact orphans  --format markdown
  ravelact extract  --format markdown
  ravelact dedup    --format markdown
} >> "$GITHUB_STEP_SUMMARY"
```

**CI gate on permissions and secrets** (fail PR on findings):

```sh
ravelact permissions
ravelact secrets
```
