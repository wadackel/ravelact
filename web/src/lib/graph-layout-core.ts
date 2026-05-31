// Core dagre-layout primitives. Lives outside both `graph-layout.ts`
// (the async wrapper that knows about the Web Worker) and
// `graph-layout.worker.ts` (the worker entrypoint) so that:
//
//   - graph-layout.worker.ts → graph-layout-core.ts  (sync function only)
//   - graph-layout.ts        → graph-layout-core.ts + graph-layout.worker.ts?worker
//
// breaks the logical cycle the previous "worker imports back from
// graph-layout.ts which imports the `?worker` virtual module"
// arrangement had. Vite's `?worker` resolution would mask the cycle at
// build time but a future bundler / Vite update could surface it.

import { type Edge, MarkerType, type Node } from "@xyflow/react";
import dagre from "dagre";
import type { CyEdgeData, CyNodeData, GraphPayload } from "./types.ts";
import { formatNodeLabel } from "./kind-format.ts";
import type { GraphNodeData } from "../ui/components/GraphNode.tsx";

// Approximate per-node box used by dagre to lay out the graph. Actual
// HTML node size is determined by CSS in `index.css`; these are the
// reservation rectangles dagre uses to assign rank/order positions.
const NODE_WIDTH = 200;
const NODE_HEIGHT = 56;

// Edge payload carried through the layout. `onDangerousPath` is set by the
// backend (CyEdgeData.on_dangerous_path) and drives the red highlight; typing
// it here lets `Graph.tsx` read it without an unchecked cast.
export type GraphEdgeData = {
  onDangerousPath: boolean;
};

export type LayoutResult = {
  nodes: Node<GraphNodeData>[];
  edges: Edge<GraphEdgeData>[];
};

// Wire protocol between graph-layout.worker.ts and computeLayout.
// The result is transferred back as an ArrayBuffer containing the
// JSON-encoded LayoutResult (D-transferable optimization at large N —
// avoids structuredClone of the result graph on the way out). Errors
// stay as plain objects since they are small.
export type WorkerResponse = { ok: true; resultBuffer: ArrayBuffer } | { ok: false; error: string };

/**
 * Compute the dagre-driven layout synchronously. This is the work the
 * Web Worker actually performs; the main thread imports it both as the
 * worker entrypoint (via graph-layout.worker.ts) and as the fallback
 * path when `globalThis.Worker` is unavailable (jsdom test environment,
 * browsers without Module Worker support, or worker-chunk delivery
 * failure).
 *
 * Throws when the payload references edge endpoints that are not present
 * in the node set. dagre would otherwise auto-create phantom nodes and
 * silently produce a wrong-looking graph, so we surface the malformed
 * payload as an exception to be caught by the calling useEffect.
 */
export function computeLayoutSync(payload: GraphPayload): LayoutResult {
  // `validatePayload` throws on malformed input and returns the unwrapped
  // `CyNodeData` / `CyEdgeData` (protobuf-es types `.data` as `T | undefined`),
  // so the layout below works with concrete data and needs no `!` assertions.
  const { nodes: nodeData, edges: edgeData } = validatePayload(payload);

  const g = new dagre.graphlib.Graph();
  g.setGraph({
    rankdir: "LR",
    nodesep: 24,
    ranksep: 80,
    edgesep: 16,
    marginx: 24,
    marginy: 24,
  });
  g.setDefaultEdgeLabel(() => ({}));

  for (const d of nodeData) {
    g.setNode(d.id, { width: NODE_WIDTH, height: NODE_HEIGHT });
  }
  for (const d of edgeData) {
    g.setEdge(d.source, d.target);
  }
  dagre.layout(g);

  const nodes: Node<GraphNodeData>[] = nodeData.map((d) => {
    const pos = g.node(d.id);
    const kind = d.kind as import("./types.ts").NodeKind;
    const { name, subtitle } = formatNodeLabel(kind, d.label);
    // Findings overlay is present only when the node actually carries
    // findings; a findings-free graph leaves `findings` undefined so the
    // card renders exactly as before.
    const fc = d.findingCounts;
    const findings: GraphNodeData["findings"] =
      fc && fc.total > 0
        ? {
            counts: {
              error: fc.error,
              high: fc.high,
              medium: fc.medium,
              low: fc.low,
              info: fc.info,
              total: fc.total,
            },
            sources: d.findingSources,
            reachableFromRisky: d.reachableFromRisky,
            isOrphan: d.isOrphan,
            hasWrite: d.hasWrite,
          }
        : undefined;
    return {
      id: d.id,
      type: "card",
      position: { x: pos.x - NODE_WIDTH / 2, y: pos.y - NODE_HEIGHT / 2 },
      data: { name, subtitle, kind, faded: false, findings },
      draggable: false,
      connectable: false,
      selectable: true,
    };
  });

  const edges: Edge<GraphEdgeData>[] = edgeData.map((d) => {
    // Dangerous-path edges are drawn red (stroke + arrowhead); markerEnd color
    // must be set per-edge here because CSS cannot reach the SVG <marker> fill.
    // The red stroke + dashed pattern are layered on in react-flow.css.
    const stroke = d.onDangerousPath ? "#dc3545" : "#3b82f6";
    return {
      id: d.id,
      source: d.source,
      target: d.target,
      type: "default",
      markerEnd: { type: MarkerType.ArrowClosed, color: stroke },
      style: { stroke: "#3b82f6", strokeWidth: 1.5 },
      // Carried so the fade effect can combine the static dangerous-path
      // class with the dynamic "faded" class on each recompute.
      data: { onDangerousPath: d.onDangerousPath },
    };
  });

  return { nodes, edges };
}

// Throws on malformed input (missing `.data`, or an edge endpoint not in the
// node set — dagre would otherwise auto-create phantom nodes). Returns the
// unwrapped `.data` arrays so the caller works with concrete data, no `!`.
function validatePayload(payload: GraphPayload): {
  nodes: CyNodeData[];
  edges: CyEdgeData[];
} {
  const ids = new Set<string>();
  const nodes: CyNodeData[] = [];
  for (const n of payload.nodes) {
    if (n.data === undefined) {
      throw new Error("graph-layout: CyNode.data missing");
    }
    ids.add(n.data.id);
    nodes.push(n.data);
  }
  const edges: CyEdgeData[] = [];
  for (const e of payload.edges) {
    if (e.data === undefined) {
      throw new Error("graph-layout: CyEdge.data missing");
    }
    if (!ids.has(e.data.source)) {
      throw new Error(`graph-layout: edge ${e.data.id} references missing source ${e.data.source}`);
    }
    if (!ids.has(e.data.target)) {
      throw new Error(`graph-layout: edge ${e.data.id} references missing target ${e.data.target}`);
    }
    edges.push(e.data);
  }
  return { nodes, edges };
}
