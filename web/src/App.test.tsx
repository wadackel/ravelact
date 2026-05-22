import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { RavelactRf } from "./lib/dev-globals.ts";

// ---------- Mocks ----------
//
// 1. api.ts: every fetch is replaced with a spy so we can drive App
//    deterministically and observe the side-effects (which endpoint
//    was hit and with what args).
// 2. Graph.tsx: ReactFlow needs viewport / ResizeObserver that jsdom
//    does not implement. Stub Graph with a tiny component that exposes
//    every prop it received to the DOM (via data-* attrs) AND lets
//    the test trigger `onNodeClick` / `onBackgroundTap` from the
//    outside through the same global test hook the real Graph uses.

vi.mock("./lib/api.ts", () => ({
  fetchGraph: vi.fn(),
  fetchTriggers: vi.fn(),
  fetchRepo: vi.fn(),
  fetchSearch: vi.fn(),
  fetchEventImpact: vi.fn(),
  fetchNode: vi.fn(),
  fetchImpact: vi.fn(),
  fetchTrace: vi.fn(),
}));

vi.mock("./ui/components/Graph.tsx", () => {
  type GraphStubProps = {
    onNodeClick: (id: string, kind: string) => void;
    onBackgroundTap: () => void;
    selectedId: string | null;
    matchedIds: Set<string> | null;
    analysisIds: Set<string> | null;
  };
  function setLatestProps(p: GraphStubProps) {
    type GlobalHook = { __testGraphLatestProps?: GraphStubProps };
    (globalThis as GlobalHook).__testGraphLatestProps = p;
  }
  function Graph(props: GraphStubProps) {
    setLatestProps(props);
    // Surface the props the real Graph would react to, so tests can
    // assert on the orchestration output without invoking ReactFlow.
    return (
      <div
        data-testid="graph-stub"
        data-selected={props.selectedId ?? ""}
        data-matched-size={props.matchedIds?.size ?? "null"}
        data-analysis-size={props.analysisIds?.size ?? "null"}
      />
    );
  }
  return { Graph };
});

import * as api from "./lib/api.ts";
import { App } from "./App.tsx";

type GraphStubProps = {
  onNodeClick: (id: string, kind: string) => void;
  onBackgroundTap: () => void;
  selectedId: string | null;
  matchedIds: Set<string> | null;
  analysisIds: Set<string> | null;
};

function latestGraphProps(): GraphStubProps {
  type GlobalHook = { __testGraphLatestProps?: GraphStubProps };
  const v = (globalThis as GlobalHook).__testGraphLatestProps;
  if (!v) throw new Error("Graph stub has not rendered yet");
  return v;
}

const GRAPH_PAYLOAD = {
  nodes: [
    {
      data: {
        id: "wf:.github/workflows/ci.yaml",
        label: "CI",
        kind: "workflow" as const,
      },
    },
    {
      data: {
        id: "wf:.github/workflows/release.yaml",
        label: "Release",
        kind: "workflow" as const,
      },
    },
  ],
  edges: [],
};

const TRIGGERS_RESPONSE = {
  rows: [
    {
      event: "push",
      entry_workflows: 1,
      declarations: 1,
      typed: 0,
      filtered: 0,
      examples: [],
    },
    {
      event: "pull_request",
      entry_workflows: 1,
      declarations: 1,
      typed: 0,
      filtered: 0,
      examples: [],
    },
  ],
};

describe("App — orchestration", () => {
  beforeEach(() => {
    (api.fetchGraph as ReturnType<typeof vi.fn>).mockResolvedValue(GRAPH_PAYLOAD);
    (api.fetchTriggers as ReturnType<typeof vi.fn>).mockResolvedValue(TRIGGERS_RESPONSE);
    (api.fetchSearch as ReturnType<typeof vi.fn>).mockResolvedValue({
      matches: [
        {
          id: "wf:.github/workflows/ci.yaml",
          kind: "workflow",
          label: "CI",
        },
      ],
      truncated: false,
      total: 1,
    });
    (api.fetchEventImpact as ReturnType<typeof vi.fn>).mockResolvedValue({
      event: "pull_request",
      entry_workflows: ["wf:.github/workflows/ci.yaml"],
      node_ids: ["wf:.github/workflows/ci.yaml"],
    });
    (api.fetchRepo as ReturnType<typeof vi.fn>).mockResolvedValue(null);
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValue({
      id: "wf:.github/workflows/ci.yaml",
      kind: "workflow",
      label: "CI",
      file: ".github/workflows/ci.yaml",
      summary: "1 job, 1 trigger",
      entry_triggers: ["push"],
      refs_in: [],
      refs_out: [],
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    type GlobalHook = {
      __testGraphLatestProps?: unknown;
      __ravelactRf?: RavelactRf;
    };
    delete (globalThis as GlobalHook).__testGraphLatestProps;
    delete (globalThis as GlobalHook).__ravelactRf;
  });

  it("fans out fetchGraph and fetchTriggers once on mount", async () => {
    render(<App />);
    await waitFor(() => {
      expect(api.fetchGraph).toHaveBeenCalledTimes(1);
      expect(api.fetchTriggers).toHaveBeenCalledTimes(1);
    });
  });

  it("OverviewPane renders by default; node click swaps it to Panel", async () => {
    render(<App />);
    await screen.findByRole("complementary", { name: "Graph overview" });
    // No node detail panel mounted yet.
    expect(screen.queryByRole("complementary", { name: "Node detail panel" })).toBeNull();

    // Simulate a node click via the Graph stub's captured callback.
    // Wrap in `act` because invoking the prop directly bypasses React's
    // event delegation, and the state update otherwise lands outside an
    // act batch.
    act(() => {
      latestGraphProps().onNodeClick("wf:.github/workflows/ci.yaml", "workflow");
    });
    await waitFor(() => {
      expect(screen.queryByRole("complementary", { name: "Graph overview" })).toBeNull();
    });
    expect(screen.getByRole("complementary", { name: "Node detail panel" })).toBeVisible();
  });

  it("typing in the search input drives matchedIds (after debounce)", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("complementary", { name: "Graph overview" });
    const input = screen.getByLabelText("Search nodes, files, and triggers");
    await user.type(input, "ci");

    await waitFor(
      () => {
        expect(api.fetchSearch).toHaveBeenCalled();
        const props = latestGraphProps();
        expect(props.matchedIds).not.toBeNull();
        expect(props.matchedIds?.has("wf:.github/workflows/ci.yaml")).toBe(true);
      },
      { timeout: 2000 },
    );
    expect(api.fetchSearch).toHaveBeenLastCalledWith("ci", expect.any(AbortSignal));
  });

  it("clearing the search input drops matchedIds back to null", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByRole("complementary", { name: "Graph overview" });
    const input = screen.getByLabelText("Search nodes, files, and triggers");
    await user.type(input, "ci");
    await waitFor(() => expect(latestGraphProps().matchedIds).not.toBeNull());
    await user.clear(input);
    await waitFor(() => expect(latestGraphProps().matchedIds).toBeNull());
  });

  it("selecting an overview event drives analysisIds; re-click toggles off", async () => {
    const user = userEvent.setup();
    render(<App />);
    const row = await screen.findByRole("button", { name: /^pull_request/ });
    await user.click(row);
    await waitFor(() => {
      expect(api.fetchEventImpact).toHaveBeenCalledWith("pull_request", expect.any(AbortSignal));
      expect(latestGraphProps().analysisIds).not.toBeNull();
    });

    // Re-click → toggle off.
    await user.click(row);
    await waitFor(() => expect(latestGraphProps().analysisIds).toBeNull());
  });

  it("keeps the active panel tab when selecting a different node", async () => {
    render(<App />);
    await screen.findByRole("complementary", { name: "Graph overview" });

    // Select node A → Panel mounts → switch to Triggers.
    act(() => {
      latestGraphProps().onNodeClick("wf:.github/workflows/ci.yaml", "workflow");
    });
    await screen.findByRole("complementary", { name: "Node detail panel" });
    const triggersTab = await screen.findByRole("tab", { name: "Triggers" });
    act(() => {
      triggersTab.click();
    });
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "Triggers" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });

    // Select node B → Panel remounts via key={selected.id}, but the
    // active tab is owned by App and survives the change.
    act(() => {
      latestGraphProps().onNodeClick("wf:.github/workflows/release.yaml", "workflow");
    });
    await waitFor(() => {
      expect(api.fetchNode).toHaveBeenCalledWith("workflow", ".github/workflows/release.yaml");
    });
    expect(screen.getByRole("tab", { name: "Triggers" })).toHaveAttribute("aria-selected", "true");
  });

  it("resets the active panel tab to Details when the panel is closed", async () => {
    render(<App />);
    await screen.findByRole("complementary", { name: "Graph overview" });

    // Select node, switch to Triggers.
    act(() => {
      latestGraphProps().onNodeClick("wf:.github/workflows/ci.yaml", "workflow");
    });
    const triggersTab = await screen.findByRole("tab", { name: "Triggers" });
    act(() => {
      triggersTab.click();
    });
    await waitFor(() => {
      expect(screen.getByRole("tab", { name: "Triggers" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });

    // Background tap closes the panel (OverviewPane comes back).
    act(() => {
      latestGraphProps().onBackgroundTap();
    });
    await screen.findByRole("complementary", { name: "Graph overview" });

    // Re-open on any node — should start on Details.
    act(() => {
      latestGraphProps().onNodeClick("wf:.github/workflows/ci.yaml", "workflow");
    });
    const detailsTab = await screen.findByRole("tab", { name: "Details" });
    expect(detailsTab).toHaveAttribute("aria-selected", "true");
  });

  it("Enter in the search input calls __ravelactRf.fitNodes with the matched ids", async () => {
    const fitNodes = vi.fn();
    // The full RavelactRf surface is large; we only stub the one method
    // the handleSearchEnter callback uses, so cast through unknown.
    (globalThis as { __ravelactRf?: RavelactRf }).__ravelactRf = {
      fitNodes,
    } as unknown as RavelactRf;

    const user = userEvent.setup();
    render(<App />);
    const input = await screen.findByLabelText("Search nodes, files, and triggers");
    await user.type(input, "ci");
    await waitFor(() => expect(api.fetchSearch).toHaveBeenCalled());

    await user.keyboard("{Enter}");
    expect(fitNodes).toHaveBeenCalledWith(["wf:.github/workflows/ci.yaml"]);
  });
});
