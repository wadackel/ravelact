# `completion <shell>`

Print a setup snippet for `bash`, `zsh`, or `fish` shell completion. The user
sources the output from their rc file.

```sh
ravelact completion bash    # then: source <(COMPLETE=bash ravelact)
ravelact completion zsh
ravelact completion fish
```
