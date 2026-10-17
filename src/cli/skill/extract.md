# `extract`

Detect duplicated step sequences across workflows and composite actions, and
emit ranked composite-action extraction candidates with a sketch `action.yml`
per candidate.

- `--min-length <N>` — minimum step count for a candidate sequence (default 3).
- `--min-occurrences <N>` — minimum occurrence count to qualify (default 2).

```sh
ravelact extract
ravelact extract --min-length 5 --min-occurrences 3 --format json
```
