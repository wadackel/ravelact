import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Background,
  type Edge,
  type Node,
  type NodeMouseHandler,
  ReactFlow,
  ReactFlowProvider,
  useEdgesState,
  useNodesState,
  useReactFlow,
  useStore,
} from "@xyflow/react";
import type { GraphPayload, NodeKind } from "../../lib/types.ts";
import {
  type FindingFacets,
  findingsVisibleSet,
  isVisible,
  reachableSet,
} from "../../lib/graph-filter.ts";
import { computeLayout, type GraphEdgeData, type LayoutResult } from "../../lib/graph-layout.ts";
import { isPerfHarnessEnabled, setRavelactRf } from "../../lib/dev-globals.ts";
import { GraphNode, type GraphNodeData } from "./GraphNode.tsx";

// Narrows ReactFlow's untyped `Node.data` to our own card data shape.
// Centralised here so a future GraphNodeData change has a single site
// to update.
const asGraphNodeData = (n: { data: unknown }): GraphNodeData => n.data as GraphNodeData;

const NODE_TYPES = { card: GraphNode };

// Threshold for showing the "Computing layout..." overlay. dogfood is
// typically below this so the overlay never appears; synthetic-300+
// crosses it and the spinner shows for the actual compute window.
const LOADING_INDICATOR_DELAY_MS = 50;

export type GraphProps = {
  payload: GraphPayload;
  onNodeClick: (id: string, kind: NodeKind) => void;
  onBackgroundTap: () => void;
  // Called when the async layout pipeline (worker + main-thread retry)
  // rejects. App surfaces this through the existing ErrorBanner. When
  // omitted the error is logged and the graph stays empty.
  onLayoutError?: (message: string) => void;
  selectedId: string | null;
  // `null` = no active search (do not let the search clause drive fade).
  // An empty Set means "active search with zero hits" → everything fades.
  matchedIds: Set<string> | null;
  // `null` = no active event-impact analysis. An empty Set means the
  // selected event has no entry workflows → everything fades.
  analysisIds: Set<string> | null;
  // Findings facets (severity / source / context). Narrows the visible set
  // via AND on top of the OR-composed filters above. Optional + defaults to
  // all-inactive so a findings-free session is byte-identical to before.
  findingFacets?: FindingFacets;
};

const INACTIVE_FACETS: FindingFacets = { severities: null, sources: null, contexts: null };

function GraphInner({
  payload,
  onNodeClick,
  onBackgroundTap,
  onLayoutError,
  selectedId,
  matchedIds,
  analysisIds,
  findingFacets = INACTIVE_FACETS,
}: GraphProps) {
  const [layout, setLayout] = useState<LayoutResult | null>(null);
  const [spinnerVisible, setSpinnerVisible] = useState<boolean>(false);

  const [nodes, setNodes, onNodesChange] = useNodesState<Node<GraphNodeData>>([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge<GraphEdgeData>>([]);

  // Compute the dagre layout off the main thread (Worker) and feed the
  // result into the ReactFlow store. A 50ms timer gates the overlay so
  // dogfood-scale layouts (typically a few ms) do not flash a spinner.
  // The AbortController is wired into `computeLayout` so an unmount or
  // payload change mid-flight terminates the underlying worker rather
  // than letting it run to completion only to drop the result.
  useEffect(() => {
    const abortCtrl = new AbortController();
    setLayout(null);
    setSpinnerVisible(false);

    const spinnerTimer = setTimeout(() => {
      if (!abortCtrl.signal.aborted) setSpinnerVisible(true);
    }, LOADING_INDICATOR_DELAY_MS);

    computeLayout(payload, abortCtrl.signal)
      .then((result) => {
        if (abortCtrl.signal.aborted) return;
        clearTimeout(spinnerTimer);
        setSpinnerVisible(false);
        setLayout(result);
        setNodes(result.nodes);
        setEdges(result.edges);
      })
      .catch((err: unknown) => {
        if (abortCtrl.signal.aborted) return;
        clearTimeout(spinnerTimer);
        setSpinnerVisible(false);
        const message = err instanceof Error ? err.message : String(err);
        if (onLayoutError) {
          onLayoutError(message);
        } else {
          console.error("graph-layout failed", err);
        }
      });

    return () => {
      abortCtrl.abort();
      clearTimeout(spinnerTimer);
    };
  }, [payload, onLayoutError, setNodes, setEdges]);

  // Pre-extract the adjacency tuples used by `reachableSet`. Memoised
  // on `payload.edges` so the fade effect below depends on a stable
  // array reference and re-runs only when the underlying graph
  // topology changes — not whenever any unrelated `payload` field
  // happens to be reassigned by a future caller.
  const adjacencyEdges = useMemo(
    () => payload.edges.map((e) => ({ source: e.data!.source, target: e.data!.target })),
    [payload.edges],
  );

  // Single writer of `data.faded` / edge `className`. Both inputs —
  // search match set and click-driven reachable set — feed into one
  // OR-composed effect, mirroring the prior cytoscape single-writer
  // invariant. A node is faded only when BOTH clauses say "fade it":
  // not in the search match (when search is active) AND not in the
  // selected reachable set (when a selection is active). Skips until
  // the first layout result lands so the initial paint is not racing
  // against an empty node array.
  useEffect(() => {
    if (!layout) return;
    if (isPerfHarnessEnabled()) {
      performance.mark("perf:tap-enter");
    }

    const reachable = selectedId ? reachableSet(adjacencyEdges, selectedId) : null;

    const filters = { matchedIds, analysisIds, reachable };
    // Findings facets narrow via AND: when active, a node must also be in the
    // findings set. `null` means no facet active → no extra constraint.
    const findingsSet = findingsVisibleSet(
      layout.nodes.map((n) => ({ id: n.id, findings: n.data.findings })),
      findingFacets,
    );
    const visible = (id: string) =>
      isVisible(id, filters) && (findingsSet === null || findingsSet.has(id));

    setNodes((curr) =>
      curr.map((n) => {
        const faded = !visible(n.id);
        return faded === n.data.faded ? n : { ...n, data: { ...n.data, faded } };
      }),
    );
    setEdges((curr) =>
      curr.map((e) => {
        const faded = !(visible(e.source) && visible(e.target));
        // Combine the dynamic fade class with the static dangerous-path class
        // so dangerous edges stay highlighted across filter recomputes.
        const dangerous = e.data?.onDangerousPath;
        const className = [faded ? "faded" : "", dangerous ? "dangerous" : ""]
          .filter(Boolean)
          .join(" ");
        return e.className === className ? e : { ...e, className };
      }),
    );
  }, [
    layout,
    selectedId,
    matchedIds,
    analysisIds,
    findingFacets,
    adjacencyEdges,
    setNodes,
    setEdges,
  ]);

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
  // never returns stale snapshots. Generic parameter ties
  // `rf.getNodes()` to `Node<GraphNodeData>[]` so the test-surface
  // installer below can read `.data.kind` without an extra cast.
  // Installation is gated on `layout` so `waitForGraph` consumers
  // cannot observe an empty store between mount and the first layout
  // resolve (the Worker race).
  const rf = useReactFlow<Node<GraphNodeData>, Edge>();
  const callbacksRef = useRef({ onNodeClick, onBackgroundTap });
  callbacksRef.current = { onNodeClick, onBackgroundTap };
  useEffect(() => {
    if (!layout) return;
    return setRavelactRf({
      getNodes: () => rf.getNodes(),
      getEdges: () => rf.getEdges(),
      tapNode: (id: string) => {
        const n = rf.getNodes().find((x) => x.id === id);
        if (!n) return null;
        callbacksRef.current.onNodeClick(id, n.data.kind);
        return id;
      },
      tapFirstWorkflow: () => {
        const n = rf.getNodes().find((x) => x.data.kind === "workflow");
        if (!n) return null;
        callbacksRef.current.onNodeClick(n.id, n.data.kind);
        return n.id;
      },
      tapFirstWorkflowExcept: (excludeId: string) => {
        const n = rf.getNodes().find((x) => x.id !== excludeId && x.data.kind === "workflow");
        if (!n) return null;
        callbacksRef.current.onNodeClick(n.id, n.data.kind);
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
          .filter((n) => n.data.faded)
          .map((n) => n.id);
        const fadedEdges = rf
          .getEdges()
          .filter((e) => e.className === "faded")
          .map((e) => e.id);
        return [...fadedNodes, ...fadedEdges].sort();
      },
    });
  }, [rf, layout]);

  return (
    <>
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
        onlyRenderVisibleElements
        proOptions={{ hideAttribution: true }}
        minZoom={0.2}
        maxZoom={2}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
      >
        <ZoomAwareBackground />
      </ReactFlow>
      {spinnerVisible && (
        <div
          role="status"
          aria-live="polite"
          data-testid="graph-loading"
          className="absolute inset-0 flex items-center justify-center pointer-events-none text-fg-muted text-sm font-sans"
        >
          Computing layout…
        </div>
      )}
    </>
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
