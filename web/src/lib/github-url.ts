import type { NodeKind, RepoInfo } from "./types.ts";

// Build a "Open in GitHub" URL for a given graph node. Returns `null` when
// no link is possible — docker nodes, missing local repo info, or
// malformed external ids.
//
// Mapping (consistent with `src/cli/render/browse.rs` id formats):
//   wf:<path>                       → blob/<ref>/<path>            (local, repo.host)
//   la:<dir>                        → tree/<ref>/<dir>             (local, repo.host)
//   ea:<o>/<r>[/<sub>]@<gitref>     → tree/<gitref>[/<sub>]        (github.com)
//   ew:<o>/<r>/<path>@<gitref>      → blob/<gitref>/<path>         (github.com)
//   dk:<image>                      → null
//
// Local nodes (`workflow` / `local-action`) require `repo` (from /api/repo)
// and use `repo.host`, so github.com and GitHub Enterprise both produce a
// working link. External nodes carry owner/repo/gitref inline in their id;
// GA spec only allows external refs on github.com, so the host is hardcoded
// regardless of the local repo's host.
export function githubUrlFor(
  node: { id: string; kind: NodeKind },
  repo: RepoInfo | null,
): string | null {
  switch (node.kind) {
    case "docker":
      return null;
    case "workflow": {
      if (!repo) return null;
      const path = stripPrefix(node.id, "wf:");
      if (path === null) return null;
      return `https://${repo.host}/${repo.owner}/${repo.repo}/blob/${repo.ref}/${path}`;
    }
    case "local-action": {
      if (!repo) return null;
      const dir = stripPrefix(node.id, "la:");
      if (dir === null) return null;
      return `https://${repo.host}/${repo.owner}/${repo.repo}/tree/${repo.ref}/${dir}`;
    }
    case "external-action": {
      const parsed = parseExternalAction(node.id);
      if (!parsed) return null;
      const { owner, repo: r, subpath, gitref } = parsed;
      const suffix = subpath ? `/tree/${gitref}/${subpath}` : `/tree/${gitref}`;
      return `https://github.com/${owner}/${r}${suffix}`;
    }
    case "external-workflow": {
      const parsed = parseExternalWorkflow(node.id);
      if (!parsed) return null;
      const { owner, repo: r, path, gitref } = parsed;
      return `https://github.com/${owner}/${r}/blob/${gitref}/${path}`;
    }
  }
}

function stripPrefix(s: string, prefix: string): string | null {
  return s.startsWith(prefix) ? s.slice(prefix.length) : null;
}

// `ea:owner/repo[/subpath]@gitref`. Split on the LAST `@` so a gitref that
// (hypothetically) contains `@` would still parse; GA spec disallows it,
// but defending here keeps the function robust.
function parseExternalAction(id: string): {
  owner: string;
  repo: string;
  subpath: string | null;
  gitref: string;
} | null {
  const body = stripPrefix(id, "ea:");
  if (body === null) return null;
  const at = body.lastIndexOf("@");
  if (at <= 0 || at === body.length - 1) return null;
  const ref = body.slice(at + 1);
  const path = body.slice(0, at);
  const firstSlash = path.indexOf("/");
  if (firstSlash <= 0) return null;
  const owner = path.slice(0, firstSlash);
  const rest = path.slice(firstSlash + 1);
  const secondSlash = rest.indexOf("/");
  const repo = secondSlash < 0 ? rest : rest.slice(0, secondSlash);
  const subpath = secondSlash < 0 ? null : rest.slice(secondSlash + 1);
  if (owner === "" || repo === "") return null;
  return { owner, repo, subpath, gitref: ref };
}

// `ew:owner/repo/path@gitref` — `path` always present (external workflows
// always reference a file).
function parseExternalWorkflow(id: string): {
  owner: string;
  repo: string;
  path: string;
  gitref: string;
} | null {
  const body = stripPrefix(id, "ew:");
  if (body === null) return null;
  const at = body.lastIndexOf("@");
  if (at <= 0 || at === body.length - 1) return null;
  const ref = body.slice(at + 1);
  const head = body.slice(0, at);
  const firstSlash = head.indexOf("/");
  if (firstSlash <= 0) return null;
  const afterOwner = head.slice(firstSlash + 1);
  const secondSlash = afterOwner.indexOf("/");
  if (secondSlash <= 0) return null;
  const owner = head.slice(0, firstSlash);
  const repo = afterOwner.slice(0, secondSlash);
  const path = afterOwner.slice(secondSlash + 1);
  if (owner === "" || repo === "" || path === "") return null;
  return { owner, repo, path, gitref: ref };
}
