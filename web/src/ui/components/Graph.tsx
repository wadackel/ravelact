import { useCallback, useEffect, useMemo, useRef } from "react";
import {
  Background,
  type Edge,
  MarkerType,
  type Node,
  type NodeMouseHandler,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  useReactFlow,
  useStore,
} from "@xyflow/react";
import dagre from "dagre";
import type { GraphPayload, NodeKind } from "../../lib/types.ts";
import { formatNodeLabel } from "../../lib/kind-format.ts";
import { isVisible, reachableSet } from "../../lib/graph-filter.ts";
import { isPerfHarnessEnabled, setRavelactRf } from "../../lib/dev-globals.ts";
import { GraphNode, type GraphNodeData } from "./GraphNode.tsx";

// Narrows ReactFlow's untyped `Node.data` to our own card data shape.
// Centralised here so a future GraphNodeData change has a single site
// to update.
const asGraphNodeData = (n: { data: unknown }): GraphNodeData => n.data as GraphNodeData;

// Approximate per-node box used by dagre to lay out the graph. Actual
// HTML node size is determined by CSS in `index.css`; these are the
// reservation rectangles dagre uses to assign rank/order positions.
const NODE_WIDTH = 200;
const NODE_HEIGHT = 56;

const NODE_TYPES = { card: GraphNode };

export type GraphProps = {
  payload: GraphPayload;
  onNodeClick: (id: string, kind: NodeKind) => void;
  onBackgroundTap: () => void;
  selectedId: string | null;
  // `null` = no active search (do not let the search clause drive fade).
  // An empty Set means "active search with zero hits" → everything fades.
  matchedIds: Set<string> | null;
  // `null` = no active event-impact analysis. An empty Set means the
  // selected event has no entry workflows → everything fades.
  analysisIds: Set<string> | null;
};

function buildLayout(payload: GraphPayload): {
  nodes: Node<GraphNodeData>[];
  edges: Edge[];
} {
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

  for (const n of payload.nodes) {
    g.setNode(n.data.id, { width: NODE_WIDTH, height: NODE_HEIGHT });
  }
  for (const e of payload.edges) {
    g.setEdge(e.data.source, e.data.target);
  }
  dagre.layout(g);

  const nodes: Node<GraphNodeData>[] = payload.nodes.map((n) => {
    const pos = g.node(n.data.id);
    const { name, subtitle } = formatNodeLabel(n.data.kind, n.data.label);
    return {
      id: n.data.id,
      type: "card",
      position: { x: pos.x - NODE_WIDTH / 2, y: pos.y - NODE_HEIGHT / 2 },
      data: { name, subtitle, kind: n.data.kind, faded: false },
      draggable: false,
      connectable: false,
      selectable: true,
    };
  });

  const edges: Edge[] = payload.edges.map((e) => ({
    id: e.data.id,
    source: e.data.source,
    target: e.data.target,
    type: "default",
    markerEnd: { type: MarkerType.ArrowClosed, color: "#3b82f6" },
    style: { stroke: "#3b82f6", strokeWidth: 1.5 },
  }));

  return { nodes, edges };
}

function GraphInner({
  payload,
  onNodeClick,
  onBackgroundTap,
  selectedId,
  matchedIds,
  analysisIds,
}: GraphProps) {
  const { nodes: initialNodes, edges: initialEdges } = useMemo(
    () => buildLayout(payload),
    [payload],
  );

  const [nodes, setNodes, onNodesChange] = useNodesState(initialNodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(initialEdges);

  // Single writer of `data.faded` / edge `className`. Both inputs —
  // search match set and click-driven reachable set — feed into one
  // OR-composed effect, mirroring the prior cytoscape single-writer
  // invariant. A node is faded only when BOTH clauses say "fade it":
  // not in the search match (when search is active) AND not in the
  // selected reachable set (when a selection is active).
  useEffect(() => {
    if (isPerfHarnessEnabled()) {
      performance.mark("perf:tap-enter");
    }

    const reachable = selectedId
      ? reachableSet(
          payload.edges.map((e) => ({
            source: e.data.source,
            target: e.data.target,
          })),
          selectedId,
        )
      : null;

    const filters = { matchedIds, analysisIds, reachable };
    const visible = (id: string) => isVisible(id, filters);

    setNodes((curr) =>
      curr.map((n) => {
        const faded = !visible(n.id);
        return faded === n.data.faded ? n : { ...n, data: { ...n.data, faded } };
      }),
    );
    setEdges((curr) =>
      curr.map((e) => {
        const faded = !(visible(e.source) && visible(e.target));
        const className = faded ? "faded" : "";
        return e.className === className ? e : { ...e, className };
      }),
    );
  }, [selectedId, matchedIds, analysisIds, payload, setNodes, setEdges]);

  // Perf probe: mark after the commit that contains the new faded
  // state. `nodes` identity changes per setNodes call.
  useEffect(() => {
    if (isPerfHarnessEnabled()) {
      performance.mark("perf:faded-applied");
    }
  }, [nodes]);

  const onPaneClick = useCallback(() => {
    onBackgroundTap();
  }, [onBackgroundTap]);

  const onNodeClickRf: NodeMouseHandler = useCallback(
    (_, node) => {
      onNodeClick(node.id, asGraphNodeData(node).kind);
    },
    [onNodeClick],
  );

  // Expose a test surface for e2e + the perf harness. Reads live
  // state from the ReactFlow store via `useReactFlow` so the hook
  // never returns stale snapshots.
  const rf = useReactFlow();
  // Keep the latest callbacks in a ref so the effect can read them
  // without subscribing — installing the global handle should run once
  // per ReactFlow instance, not on every parent re-render that
  // produces a fresh inline arrow.
  const callbacksRef = useRef({ onNodeClick, onBackgroundTap });
  callbacksRef.current = { onNodeClick, onBackgroundTap };
  useEffect(() => {
    return setRavelactRf({
      getNodes: () => rf.getNodes(),
      getEdges: () => rf.getEdges(),
      tapNode: (id: string) => {
        const n = rf.getNodes().find((x) => x.id === id);
        if (!n) return null;
        callbacksRef.current.onNodeClick(id, asGraphNodeData(n).kind);
        return id;
      },
      tapFirstWorkflow: () => {
        const n = rf.getNodes().find((x) => asGraphNodeData(x).kind === "workflow");
        if (!n) return null;
        callbacksRef.current.onNodeClick(n.id, asGraphNodeData(n).kind);
        return n.id;
      },
      tapFirstWorkflowExcept: (excludeId: string) => {
        const n = rf
          .getNodes()
          .find((x) => x.id !== excludeId && asGraphNodeData(x).kind === "workflow");
        if (!n) return null;
        callbacksRef.current.onNodeClick(n.id, asGraphNodeData(n).kind);
        return n.id;
      },
      backgroundTap: () => callbacksRef.current.onBackgroundTap(),
      // ReactFlow CSS transforms make pan effectively instant — this
      // helper exists so the perf harness can drive panning at the
      // same call site shape it used for cytoscape's `cy.panBy`.
      panBy: (dx: number, dy: number) => {
        const vp = rf.getViewport();
        rf.setViewport({ x: vp.x + dx, y: vp.y + dy, zoom: vp.zoom });
      },
      // Zoom + center on a set of node ids — used by the Enter
      // keybinding in the search input to bring all matches into view.
      fitNodes: (ids: string[]) => {
        if (!ids.length) return;
        rf.fitView({
          nodes: ids.map((id) => ({ id })),
          padding: 0.25,
          duration: 250,
        });
      },
      fadedIds: () => {
        const fadedNodes = rf
          .getNodes()
          .filter((n) => asGraphNodeData(n).faded)
          .map((n) => n.id);
        const fadedEdges = rf
          .getEdges()
          .filter((e) => e.className === "faded")
          .map((e) => e.id);
        return [...fadedNodes, ...fadedEdges].sort();
      },
    });
  }, [rf]);

  return (
    <ReactFlow
      nodes={nodes}
      edges={edges}
      nodeTypes={NODE_TYPES}
      onNodesChange={onNodesChange}
      onEdgesChange={onEdgesChange}
      onNodeClick={onNodeClickRf}
      onPaneClick={onPaneClick}
      fitView
      fitViewOptions={{ padding: 0.15 }}
      proOptions={{ hideAttribution: true }}
      minZoom={0.2}
      maxZoom={2}
      nodesDraggable={false}
      nodesConnectable={false}
      elementsSelectable
    >
      <ZoomAwareBackground />
    </ReactFlow>
  );
}

// Subscribes only to the zoom slice of the viewport so it re-renders on
// zoom but not on pan, then counter-scales gap/size so the dot pattern's
// on-screen appearance stays constant across zoom levels.
function ZoomAwareBackground() {
  const zoom = useStore((s) => s.transform[2]);
  return <Background gap={20 / zoom} size={2 / zoom} color="#d4d4d8" />;
}

export function Graph(props: GraphProps) {
  return (
    <div
      id="cy"
      className="absolute inset-0"
      role="img"
      aria-label="Workflow dependency graph (visual). Click nodes to inspect details in the side panel."
    >
      <ReactFlowProvider>
        <GraphInner {...props} />
      </ReactFlowProvider>
    </div>
  );
}
