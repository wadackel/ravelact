# `build`

Discover workflows and local actions (composite / JavaScript / Docker), build
the IR, and persist it to
`${XDG_STATE_HOME:-$HOME/.local/state}/ravelact/repo-<sha8>/cache.json`. Run
once per session before other commands; subsequent commands reuse the cache
automatically. Use `--no-cache` to force a rebuild.

```sh
ravelact build --root .
```
