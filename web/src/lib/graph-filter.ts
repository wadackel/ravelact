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
