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
import type { GraphPayload } from "./types.ts";
import { formatNodeLabel } from "./kind-format.ts";
import type { GraphNodeData } from "../ui/components/GraphNode.tsx";

// Approximate per-node box used by dagre to lay out the graph. Actual
// HTML node size is determined by CSS in `index.css`; these are the
// reservation rectangles dagre uses to assign rank/order positions.
const NODE_WIDTH = 200;
const NODE_HEIGHT = 56;

export type LayoutResult = {
  nodes: Node<GraphNodeData>[];
  edges: Edge[];
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
  validatePayload(payload);

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

  // protobuf-es types `CyNode.data` / `CyEdge.data` as
  // `MessageField<...>` which is `T | undefined` at the type level.
  // `validatePayload` above guarantees both are set on every element,
  // so the non-null assertions below are safe.
  for (const n of payload.nodes) {
    g.setNode(n.data!.id, { width: NODE_WIDTH, height: NODE_HEIGHT });
  }
  for (const e of payload.edges) {
    g.setEdge(e.data!.source, e.data!.target);
  }
  dagre.layout(g);

  const nodes: Node<GraphNodeData>[] = payload.nodes.map((n) => {
    const d = n.data!;
    const pos = g.node(d.id);
    const kind = d.kind as import("./types.ts").NodeKind;
    const { name, subtitle } = formatNodeLabel(kind, d.label);
    return {
      id: d.id,
      type: "card",
      position: { x: pos.x - NODE_WIDTH / 2, y: pos.y - NODE_HEIGHT / 2 },
      data: { name, subtitle, kind, faded: false },
      draggable: false,
      connectable: false,
      selectable: true,
    };
  });

  const edges: Edge[] = payload.edges.map((e) => {
    const d = e.data!;
    return {
      id: d.id,
      source: d.source,
      target: d.target,
      type: "default",
      markerEnd: { type: MarkerType.ArrowClosed, color: "#3b82f6" },
      style: { stroke: "#3b82f6", strokeWidth: 1.5 },
    };
  });

  return { nodes, edges };
}

function validatePayload(payload: GraphPayload): void {
  const ids = new Set<string>();
  for (const n of payload.nodes) {
    if (n.data === undefined) {
      throw new Error("graph-layout: CyNode.data missing");
    }
    ids.add(n.data.id);
  }
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
  }
}
