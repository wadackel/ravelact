import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
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
    onLayoutError?: (message: string) => void;
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

// 3. ResizableRightPane: replace with a stub that publishes its latest
//    props to a global hook so the persistence + width-propagation
//    tests can observe and drive width changes without mounting
//    re-resizable's DOM.
type ResizableStubProps = {
  width: number;
  onWidthChange: (next: number) => void;
};
vi.mock("./ui/components/ResizableRightPane.tsx", () => {
  function setLatestProps(p: ResizableStubProps) {
    type GlobalHook = { __testResizableLatestProps?: ResizableStubProps };
    (globalThis as GlobalHook).__testResizableLatestProps = p;
  }
  function ResizableRightPane(props: ResizableStubProps & { children: ReactNode }) {
    setLatestProps({ width: props.width, onWidthChange: props.onWidthChange });
    return (
      <div data-testid="resizable-stub" data-width={props.width}>
        {props.children}
      </div>
    );
  }
  return { ResizableRightPane };
});

function latestResizableProps(): ResizableStubProps {
  type GlobalHook = { __testResizableLatestProps?: ResizableStubProps };
  const v = (globalThis as GlobalHook).__testResizableLatestProps;
  if (!v) throw new Error("ResizableRightPane stub has not rendered yet");
  return v;
}

import * as api from "./lib/api.ts";
import { App } from "./App.tsx";

type GraphStubProps = {
  onNodeClick: (id: string, kind: string) => void;
  onBackgroundTap: () => void;
  onLayoutError?: (message: string) => void;
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
      if_conditions: [],
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    type GlobalHook = {
      __testGraphLatestProps?: unknown;
      __testResizableLatestProps?: unknown;
      __ravelactRf?: RavelactRf;
    };
    delete (globalThis as GlobalHook).__testGraphLatestProps;
    delete (globalThis as GlobalHook).__testResizableLatestProps;
    delete (globalThis as GlobalHook).__ravelactRf;
    localStorage.clear();
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

  it("clicking a Triggers tab chip drives event-impact through Panel", async () => {
    render(<App />);
    await screen.findByRole("complementary", { name: "Graph overview" });

    // Select a workflow node so Panel mounts in place of OverviewPane.
    act(() => {
      latestGraphProps().onNodeClick("wf:.github/workflows/ci.yaml", "workflow");
    });
    const panel = await screen.findByRole("complementary", { name: "Node detail panel" });

    // Switch to the Triggers tab. Scope the lookup to the panel so we
    // do not collide with any future OverviewPane button that might
    // share an accessible name.
    const triggersTab = within(panel).getByRole("tab", { name: "Triggers" });
    act(() => {
      triggersTab.click();
    });

    // The Triggers chip is rendered after fetchNode resolves with the
    // ci.yaml mock that has `entry_triggers: ["push"]`.
    const chip = await within(panel).findByRole("button", { name: "push" });
    expect(chip).toHaveAttribute("aria-pressed", "false");

    act(() => {
      chip.click();
    });
    await waitFor(() => {
      expect(api.fetchEventImpact).toHaveBeenCalledWith("push", expect.any(AbortSignal));
      expect(latestGraphProps().analysisIds).not.toBeNull();
    });
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

  it("mounts PoweredBy exactly once inside the graph section, even before /api/graph resolves", () => {
    // The Graph payload is stubbed but PoweredBy renders unconditionally, so
    // the credit must already be present on the very first render — before
    // any async fetch settles.
    render(<App />);
    const graphSection = screen.getByTestId("graph-section");
    const credits = within(graphSection).getAllByRole("link", {
      name: /Powered by ravelact/i,
    });
    expect(credits).toHaveLength(1);
    expect(credits[0]).toHaveAttribute("href", "https://github.com/wadackel/ravelact");
  });

  describe("right-pane width persistence", () => {
    it("starts at 360 when localStorage is empty", async () => {
      render(<App />);
      await waitFor(() => latestResizableProps());
      expect(latestResizableProps().width).toBe(360);
    });

    it("restores a persisted in-range value on mount", async () => {
      localStorage.setItem("ravelact:panel-width", "500");
      render(<App />);
      await waitFor(() => latestResizableProps());
      expect(latestResizableProps().width).toBe(500);
    });

    it.each([
      ["99999", "above MAX_CAP"],
      ["-10", "negative"],
      ["abc", "non-numeric"],
      ["", "empty"],
      ["100", "below MIN"],
    ])("falls back to 360 when persisted value is %s (%s)", async (raw) => {
      localStorage.setItem("ravelact:panel-width", raw);
      render(<App />);
      await waitFor(() => latestResizableProps());
      expect(latestResizableProps().width).toBe(360);
    });

    it("onWidthChange writes through to localStorage and updates the width prop", async () => {
      render(<App />);
      await waitFor(() => latestResizableProps());
      act(() => {
        latestResizableProps().onWidthChange(450);
      });
      expect(localStorage.getItem("ravelact:panel-width")).toBe("450");
      expect(latestResizableProps().width).toBe(450);
    });

    it("preserves the width across Panel ↔ OverviewPane toggles", async () => {
      render(<App />);
      await waitFor(() => latestResizableProps());
      // Start at default 360; bump to 480.
      act(() => latestResizableProps().onWidthChange(480));
      expect(latestResizableProps().width).toBe(480);

      // OverviewPane → Panel via a node click on the graph stub.
      await waitFor(() => latestGraphProps());
      act(() => latestGraphProps().onNodeClick("wf:.github/workflows/ci.yaml", "workflow"));
      await waitFor(() => {
        expect(
          screen.getByRole("complementary", { name: "Node detail panel" }),
        ).toBeInTheDocument();
      });
      expect(latestResizableProps().width).toBe(480);

      // Panel → OverviewPane via background tap.
      act(() => latestGraphProps().onBackgroundTap());
      await waitFor(() => {
        expect(screen.getByRole("complementary", { name: "Graph overview" })).toBeInTheDocument();
      });
      expect(latestResizableProps().width).toBe(480);
    });
  });
});
