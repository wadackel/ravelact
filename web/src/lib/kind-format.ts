import type { NodeKind } from "./types.ts";

// Canonical runtime list of NodeKind variants. Use this — never inline
// a fresh `["workflow", ...]` literal — when iterating over kinds in
// app code or tests. Kind colours live in the `@theme` block of
// `src/index.css` as `--color-kind-<kind>-{fill,border,text}` and are
// applied via the `data-kind` attribute selector; no runtime palette
// object is needed.
export const NODE_KINDS: ReadonlyArray<NodeKind> = [
  "workflow",
  "local-action",
  "external-action",
  "external-workflow",
  "docker",
];

const KIND_BADGE: Record<NodeKind, string> = {
  workflow: "WF",
  "local-action": "LA",
  "external-action": "EA",
  "external-workflow": "EW",
  docker: "DK",
};

export function kindBadge(kind: NodeKind): string {
  return KIND_BADGE[kind];
}

const KIND_LABEL: Record<NodeKind, string> = {
  workflow: "Workflow",
  "local-action": "Local action",
  "external-action": "External action",
  "external-workflow": "External workflow",
  docker: "Docker",
};

export function kindLabel(kind: NodeKind): string {
  return KIND_LABEL[kind];
}

const SHA_RE = /^[0-9a-f]{40}$/i;

function shortRef(ref: string): string {
  return SHA_RE.test(ref) ? ref.slice(0, 7) : ref;
}

// Two-line label for cytoscape node rendering. The Rust browse builder
// emits long labels like `actions/checkout@de0fac...<full-sha>` for
// external actions; here we split them into a primary name + dim
// subtitle so each card stays compact and matches the PNG reference.
export function formatNodeLabel(kind: NodeKind, label: string): { name: string; subtitle: string } {
  if (kind === "external-action" || kind === "external-workflow") {
    const at = label.lastIndexOf("@");
    if (at > 0 && at < label.length - 1) {
      return {
        name: label.slice(0, at),
        subtitle: `@${shortRef(label.slice(at + 1))}`,
      };
    }
    return { name: label, subtitle: kindLabel(kind).toLowerCase() };
  }
  if (kind === "docker") {
    const colon = label.lastIndexOf(":");
    if (colon > 0 && colon < label.length - 1) {
      return {
        name: label.slice(0, colon),
        subtitle: `:${label.slice(colon + 1)}`,
      };
    }
    return { name: label, subtitle: "docker" };
  }
  if (kind === "workflow") {
    return { name: label, subtitle: "workflow" };
  }
  // local-action
  return { name: label, subtitle: "local action" };
}
