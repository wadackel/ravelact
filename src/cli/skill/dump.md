# `dump`

Print the entire IR as JSON on stdout. Always JSON; no `--format` flag.
Useful when piping into `jq` for custom queries beyond what the built-in
commands offer.

```sh
ravelact dump | jq '.workflows[] | select(.triggers[].event == "push")'
```
