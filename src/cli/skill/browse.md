# `browse`

Spawn a local HTTP server that bundles a React SPA over the IR and opens it
in the default browser. The page renders the workflow estate as an
interactive graph: nodes are workflows / local-actions / external-actions /
docker images, edges follow `uses` / `workflow_call`, and a side panel
shows per-node details (file, triggers, impact, trace) without leaving the
keyboard. Useful for ad-hoc exploration where a CLI table would be too
dense — e.g., visualizing which external actions a `push` event reaches.

- `--port <port>` — bind to a specific port (default: auto-assigned).
- `--no-open` — do not launch the browser; print the URL only.
- `--include-test-fixtures` — include local-action manifests under
  `tests/fixtures/**` (default: excluded to keep the dogfood view focused
  on production workflows).
- `--findings <PATH>` — overlay external SARIF findings in the UI (see
  `ravelact skill findings`).
- `/api/repo` returns 200 when `origin` resolves to a GitHub-like host
  (github.com or GitHub Enterprise) over `ssh://`, `git@host:` (SCP form),
  `https://`, or `http://`; userinfo and ports are tolerated. It returns
  404 only when the directory is not a git repo, has no `origin` remote,
  or `origin` uses an unsupported scheme (e.g. `git://`, `file://`). The
  frontend hides the "Open in GitHub" link for local nodes when 404.

```sh
ravelact browse                          # opens http://127.0.0.1:<auto>/
ravelact browse --no-open --port 7878    # CI / agent friendly
```
