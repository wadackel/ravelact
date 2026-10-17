# `dedup`

Cluster near-duplicate workflows by structural + run-script similarity using
weighted-Jaccard similarity and single-linkage union-find. Outputs each
cluster's representative, members, common/divergent `uses:` references, and
whether the trigger sets differ.

- `--threshold <F>` — similarity threshold in `[0, 1]` for clustering (default
  `0.8`). Higher = stricter (fewer clusters, tighter members).

```sh
ravelact dedup
ravelact dedup --threshold 0.9 --format json | jq 'length'
```

A common CI gate is "no near-duplicate clusters at threshold X":

```sh
ravelact dedup --format json | jq -e 'length == 0'
```
