// Type definitions mirroring the JSON shapes emitted by `src/cli/render/browse.rs`.
//
// The `NodeKind` and `EdgeKind` string literals must match the Rust source
// verbatim (see `add_node` / `add_edge` call sites in `src/cli/render/browse.rs`).
// `TraceJsonNode` mirrors the `#[serde(tag = "kind", rename_all = "kebab-case")]`
// enum declared in `src/query/trace.rs`.

export type NodeKind =
  | "workflow"
  | "local-action"
  | "external-action"
  | "external-workflow"
  | "docker";

export type EdgeKind =
  | "annotation"
  | "calls-workflow"
  | "uses-local-workflow"
  | "uses-local-action"
  | "uses-external-action"
  | "uses-docker";

// ----- /api/graph -----

export type CyNode = {
  data: { id: string; label: string; kind: NodeKind };
};

export type CyEdge = {
  data: { id: string; source: string; target: string; kind: EdgeKind };
};

export type GraphPayload = {
  nodes: CyNode[];
  edges: CyEdge[];
};

// ----- /api/triggers -----

export type TriggerSummary = {
  event: string;
  entry_workflows: number;
  declarations: number;
  typed: number;
  filtered: number;
  examples: string[];
};

export type TriggersResponse = {
  rows: TriggerSummary[];
};

// ----- /api/node -----

/**
 * Subset of `NodeKind` that the `/api/node` endpoint can return. The Rust
 * handler returns `404` for `external-workflow` and `docker` (they are not
 * first-class IR collections), so the response can only carry one of these
 * three. Keeping the type narrower preserves exhaustive switch coverage.
 */
export type NodeResponseKind = "workflow" | "local-action" | "external-action";

export type NodeResponse = {
  id: string;
  kind: NodeResponseKind;
  label: string;
  file: string;
  summary: string;
  entry_triggers: string[];
  refs_in: string[];
  refs_out: string[];
};

// ----- /api/impact -----

/**
 * Mirrors Rust's `ActionKind` enum (composite / javascript / docker) as a
 * discriminated union for type-safe display logic.
 */
export type ActionKind = "composite" | "javascript" | "docker";

export type ImpactAction = {
  id: string;
  kind: ActionKind;
};

export type ImpactResponse = {
  workflows: string[];
  actions: ImpactAction[];
  unknowns: string[];
};

// ----- /api/trace -----

export type TraceJsonNode =
  | { kind: "workflow"; id: string; children: TraceJsonNode[] }
  | { kind: "action"; id: string; children: TraceJsonNode[] }
  | {
      kind: "external-action";
      owner: string;
      repo: string;
      subpath?: string;
      gitref: string;
    }
  | {
      kind: "external-workflow";
      owner: string;
      repo: string;
      path: string;
      gitref: string;
    }
  | { kind: "docker"; image: string }
  | {
      kind: "annotated";
      verb: string;
      dangling: boolean;
      label: string;
      children: TraceJsonNode[];
    }
  | { kind: "cycle"; target_kind: string; target: string }
  | { kind: "guarded"; if_expr: string; inner: TraceJsonNode };

export type TraceResponse = {
  tree: TraceJsonNode;
  event_used: string;
};

// ----- /api/search -----

export type SearchMatch = {
  id: string;
  kind: string;
  label: string;
};

export type SearchResponse = {
  matches: SearchMatch[];
  truncated: boolean;
  total: number;
};

// ----- /api/event-impact -----

export type EventImpactResponse = {
  event: string;
  entry_workflows: string[];
  node_ids: string[];
};

// ----- /api/repo -----

/**
 * GitHub provenance of the local repository served by `browse`. Computed
 * once at server startup from `git remote get-url origin` + the current
 * branch (falling back to HEAD SHA when detached). The endpoint returns
 * 404 — surfaced here as `null` — when the root is not a git repo, has
 * no `origin`, points at a non-GitHub host, or has neither a branch nor
 * a SHA. The frontend uses this to construct "Open in GitHub" deep links
 * for `workflow` / `local-action` nodes.
 */
export type RepoInfo = {
  host: string;
  owner: string;
  repo: string;
  ref: string;
};
