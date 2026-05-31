// Pure helpers for the graph fade rule. Kept renderer-agnostic so they
// can be unit-tested without React / ReactFlow. `Graph.tsx` imports
// these and applies them inside its single-writer useEffect.

export type EdgeRef = { source: string; target: string };

/**
 * Set of node ids reachable from `rootId` along the directed edges, in
 * BOTH directions (predecessors ∪ successors ∪ self). Mirrors the
 * original cytoscape `node.predecessors().union(node.successors())`
 * semantics so the fade highlight behavior stays identical after the
 * renderer swap.
 */
export function reachableSet(edges: EdgeRef[], rootId: string): Set<string> {
  const fwd = new Map<string, string[]>();
  const rev = new Map<string, string[]>();
  for (const e of edges) {
    (fwd.get(e.source) ?? fwd.set(e.source, []).get(e.source)!).push(e.target);
    (rev.get(e.target) ?? rev.set(e.target, []).get(e.target)!).push(e.source);
  }
  const out = new Set<string>([rootId]);
  const queueFwd = [rootId];
  while (queueFwd.length) {
    const n = queueFwd.shift()!;
    for (const next of fwd.get(n) ?? []) {
      if (!out.has(next)) {
        out.add(next);
        queueFwd.push(next);
      }
    }
  }
  const queueRev = [rootId];
  while (queueRev.length) {
    const n = queueRev.shift()!;
    for (const prev of rev.get(n) ?? []) {
      if (!out.has(prev)) {
        out.add(prev);
        queueRev.push(prev);
      }
    }
  }
  return out;
}

export type VisibilityFilters = {
  // `null` = filter is inactive (contributes no constraint).
  // An empty Set = filter active with zero hits → nothing visible.
  matchedIds: Set<string> | null;
  analysisIds: Set<string> | null;
  reachable: Set<string> | null;
};

/**
 * OR-composes every ACTIVE filter. A node is visible iff:
 *
 *   - no filter is active (all three are null), OR
 *   - the node appears in at least one active filter set.
 *
 * Active here means "not null". An empty Set is active (and excludes
 * every node) — that is how "active search with zero hits" fades the
 * whole graph instead of silently doing nothing.
 */
export function isVisible(id: string, filters: VisibilityFilters): boolean {
  const { matchedIds, analysisIds, reachable } = filters;
  if (matchedIds === null && analysisIds === null && reachable === null) {
    return true;
  }
  if (matchedIds !== null && matchedIds.has(id)) return true;
  if (analysisIds !== null && analysisIds.has(id)) return true;
  if (reachable !== null && reachable.has(id)) return true;
  return false;
}

// ---------------------------------------------------------------------------
// Findings facets (separate AND dimension layered on top of `isVisible`).
//
// Unlike the OR-composed `isVisible` filters, the findings facets NARROW the
// visible set: a node must pass EVERY active facet (severity AND source AND
// context). `Graph.tsx` composes the two — a node is visible iff
// `isVisible(id) && (findingsSet === null || findingsSet.has(id))`. Keeping
// the OR model untouched means existing search / analysis behavior is
// unchanged; findings filtering is purely additive.
// ---------------------------------------------------------------------------

export type FindingContext = "reachable" | "orphan" | "write";

// Per-node finding metadata the facets test against. Structural subset of
// `GraphNodeData["findings"]` so this module stays renderer-agnostic.
export type NodeFindingMeta = {
  counts: { error: number; high: number; medium: number; low: number; info: number };
  sources: readonly string[];
  reachableFromRisky: boolean;
  isOrphan: boolean;
  hasWrite: boolean;
};

// Each facet is `null` when inactive (no constraint) or a Set of selected
// values. Within a facet the match is OR (any selected value); facets AND
// together.
export type FindingFacets = {
  severities: ReadonlySet<string> | null;
  sources: ReadonlySet<string> | null;
  contexts: ReadonlySet<FindingContext> | null;
};

export function findingsActive(f: FindingFacets): boolean {
  return f.severities !== null || f.sources !== null || f.contexts !== null;
}

function hasAnySeverity(c: NodeFindingMeta["counts"], sevs: ReadonlySet<string>): boolean {
  return (
    (sevs.has("error") && c.error > 0) ||
    (sevs.has("high") && c.high > 0) ||
    (sevs.has("medium") && c.medium > 0) ||
    (sevs.has("low") && c.low > 0) ||
    (sevs.has("info") && c.info > 0)
  );
}

function matchesContext(m: NodeFindingMeta, ctx: ReadonlySet<FindingContext>): boolean {
  return (
    (ctx.has("reachable") && m.reachableFromRisky) ||
    (ctx.has("orphan") && m.isOrphan) ||
    (ctx.has("write") && m.hasWrite)
  );
}

/**
 * Set of node ids that pass every active findings facet, or `null` when no
 * facet is active (so the caller skips the AND and leaves `isVisible` alone).
 * Only finding-bearing nodes can pass; a node with no findings is excluded
 * whenever any facet is active.
 */
export function findingsVisibleSet(
  nodes: ReadonlyArray<{ id: string; findings?: NodeFindingMeta }>,
  facets: FindingFacets,
): Set<string> | null {
  if (!findingsActive(facets)) return null;
  // Capture into locals so the per-facet checks keep their narrowing inside
  // the loop / closure without a non-null assertion.
  const { severities, sources, contexts } = facets;
  const out = new Set<string>();
  for (const n of nodes) {
    const m = n.findings;
    if (!m) continue;
    if (severities && !hasAnySeverity(m.counts, severities)) continue;
    if (sources && !m.sources.some((s) => sources.has(s))) continue;
    if (contexts && !matchesContext(m, contexts)) continue;
    out.add(n.id);
  }
  return out;
}
