import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { type ReactNode } from "react";
import type { GraphPayload } from "../../lib/types.ts";
import type { LayoutResult } from "../../lib/graph-layout.ts";

// Mock ReactFlow surface so jsdom doesn't choke on viewport / ResizeObserver
// internals. We only verify the loading-overlay timing + `__ravelactRf`
// install timing here; the real renderer is exercised by e2e. The
// `useReactFlow` return value is stable across renders so the install
// effect's `[rf, layout]` deps do not churn between commits.
const stableRf = {
  getNodes: () => [],
  getEdges: () => [],
  getViewport: () => ({ x: 0, y: 0, zoom: 1 }),
  setViewport: () => {},
  fitView: () => {},
};
vi.mock("@xyflow/react", () => {
  return {
    ReactFlowProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
    ReactFlow: ({ children }: { children?: ReactNode }) => (
      <div data-testid="rf-mock">{children}</div>
    ),
    Background: () => null,
    MarkerType: { ArrowClosed: "arrowclosed" },
    useNodesState: () => [[], () => {}, () => {}],
    useEdgesState: () => [[], () => {}, () => {}],
    useReactFlow: () => stableRf,
    useStore: () => 1,
  };
});

// `computeLayout` is replaced per-test by a deferred Promise so we can
// drive resolution from outside. We use real timers throughout to keep
// the fake-timer/microtask interaction out of scope — tests sleep tens
// of ms which keeps the file fast and reliable.
let layoutResolver: ((result: LayoutResult) => void) | null = null;
let layoutRejecter: ((err: Error) => void) | null = null;
function newDeferredLayout(): Promise<LayoutResult> {
  return new Promise<LayoutResult>((resolve, reject) => {
    layoutResolver = resolve;
    layoutRejecter = reject;
  });
}
vi.mock("../../lib/graph-layout.ts", () => ({
  computeLayout: vi.fn(() => newDeferredLayout()),
}));

// Spy on the dev-globals install so we can observe install/cleanup
// without depending on globalThis side-channel state.
const setRavelactRfSpy = vi.fn((_handle: Record<string, unknown>) => () => {});
vi.mock("../../lib/dev-globals.ts", () => ({
  isPerfHarnessEnabled: () => false,
  setRavelactRf: (handle: Record<string, unknown>) => setRavelactRfSpy(handle),
}));

import { Graph } from "./Graph.tsx";

const EMPTY_LAYOUT: LayoutResult = { nodes: [], edges: [] };

const PAYLOAD: GraphPayload = {
  nodes: [{ data: { id: "wf:a", label: "A", kind: "workflow" } }],
  edges: [],
};

function renderGraph(extra: { onLayoutError?: (m: string) => void } = {}) {
  return render(
    <Graph
      payload={PAYLOAD}
      onNodeClick={() => {}}
      onBackgroundTap={() => {}}
      onLayoutError={extra.onLayoutError}
      selectedId={null}
      matchedIds={null}
      analysisIds={null}
    />,
  );
}

function sleep(ms: number) {
  return new Promise<void>((r) => setTimeout(r, ms));
}

beforeEach(() => {
  layoutResolver = null;
  layoutRejecter = null;
  setRavelactRfSpy.mockClear();
});

afterEach(() => {
  cleanup();
});

describe("Graph loading overlay (50ms delay)", () => {
  it("stays hidden when computeLayout resolves before the 50ms threshold", async () => {
    renderGraph();
    expect(screen.queryByTestId("graph-loading")).toBeNull();

    // Resolve at t≈10ms (well before the 50ms timer fires).
    await sleep(10);
    layoutResolver?.(EMPTY_LAYOUT);

    // Wait long enough that the would-be-spinner timer is past its fire
    // time. If the timer were still armed after resolve, the overlay
    // would have appeared by now.
    await sleep(80);
    expect(screen.queryByTestId("graph-loading")).toBeNull();
  });

  it("appears once the 50ms threshold passes and disappears on resolve", async () => {
    renderGraph();
    expect(screen.queryByTestId("graph-loading")).toBeNull();

    // Past the threshold without resolving — overlay should appear.
    await waitFor(() => {
      expect(screen.getByTestId("graph-loading")).toBeInTheDocument();
    });

    // Resolve — overlay must disappear.
    layoutResolver?.(EMPTY_LAYOUT);
    await waitFor(() => {
      expect(screen.queryByTestId("graph-loading")).toBeNull();
    });
  });
});

describe("Graph __ravelactRf install timing", () => {
  it("does not call setRavelactRf before the first layout resolve", async () => {
    renderGraph();
    // Cross the spinner threshold to give effects plenty of room to
    // fire an unwanted install — none should happen because layout is
    // still pending.
    await sleep(80);
    expect(setRavelactRfSpy).not.toHaveBeenCalled();
  });

  it("calls setRavelactRf after the first layout resolves", async () => {
    renderGraph();
    expect(layoutResolver).not.toBeNull();
    await act(async () => {
      layoutResolver?.(EMPTY_LAYOUT);
      await new Promise<void>((r) => setTimeout(r, 0));
    });
    expect(setRavelactRfSpy).toHaveBeenCalledTimes(1);
    const handle = setRavelactRfSpy.mock.calls[0]![0];
    expect(typeof handle.getNodes).toBe("function");
    expect(typeof handle.fadedIds).toBe("function");
  });

  it("does not call setRavelactRf when layout rejects (calls onLayoutError instead)", async () => {
    const onLayoutError = vi.fn();
    renderGraph({ onLayoutError });
    await act(async () => {
      layoutRejecter?.(new Error("layout boom"));
    });
    await waitFor(() => {
      expect(onLayoutError).toHaveBeenCalledWith("layout boom");
    });
    expect(setRavelactRfSpy).not.toHaveBeenCalled();
  });
});
