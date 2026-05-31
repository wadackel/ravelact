import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

// `Handle` reads the ReactFlow store via context; stub it (and `Position`)
// so GraphNode can render standalone without a ReactFlowProvider.
vi.mock("@xyflow/react", () => ({
  Handle: () => null,
  Position: { Left: "left", Right: "right" },
}));

import {
  compactCounts,
  type FindingOverlay,
  GraphNode,
  type GraphNodeData,
  maxBadgeSeverity,
} from "./GraphNode.tsx";

function counts(over: Partial<FindingOverlay["counts"]> = {}): FindingOverlay["counts"] {
  const c = { error: 0, high: 0, medium: 0, low: 0, info: 0, ...over };
  return { ...c, total: c.error + c.high + c.medium + c.low + c.info };
}

function data(over: Partial<GraphNodeData> = {}): GraphNodeData {
  return {
    name: "ci.yml",
    subtitle: "workflow",
    kind: "workflow",
    faded: false,
    ...over,
  };
}

afterEach(cleanup);

describe("compactCounts / maxBadgeSeverity", () => {
  it("compacts non-zero tiers in severity order", () => {
    expect(compactCounts(counts({ error: 1, medium: 2 }))).toBe("E1 M2");
    expect(compactCounts(counts({ high: 3, low: 1, info: 4 }))).toBe("H3 L1 I4");
    expect(compactCounts(counts())).toBe("");
  });

  it("buckets the max severity (error|high → high, medium → medium, else low)", () => {
    expect(maxBadgeSeverity(counts({ error: 1 }))).toBe("high");
    expect(maxBadgeSeverity(counts({ high: 1 }))).toBe("high");
    expect(maxBadgeSeverity(counts({ medium: 1 }))).toBe("medium");
    expect(maxBadgeSeverity(counts({ low: 2 }))).toBe("low");
    expect(maxBadgeSeverity(counts({ info: 5 }))).toBe("low");
  });
});

describe("GraphNode finding badge", () => {
  it("renders a badge with compact counts and high severity color attr", () => {
    render(
      <GraphNode
        data={data({
          findings: {
            counts: counts({ error: 1, high: 1, medium: 1 }),
            sources: ["zizmor"],
            reachableFromRisky: true,
            isOrphan: false,
            hasWrite: true,
          },
        })}
      />,
    );
    const badge = screen.getByTestId("finding-badge");
    expect(badge).toHaveAttribute("data-severity", "high");
    expect(badge).toHaveTextContent("E1 H1 M1");
    expect(badge).toHaveAttribute("aria-label", "3 findings: E1 H1 M1");
  });

  it("uses medium color when the worst tier is medium", () => {
    render(
      <GraphNode
        data={data({
          findings: {
            counts: counts({ medium: 2, low: 1 }),
            sources: ["zizmor"],
            reachableFromRisky: false,
            isOrphan: false,
            hasWrite: false,
          },
        })}
      />,
    );
    expect(screen.getByTestId("finding-badge")).toHaveAttribute("data-severity", "medium");
  });

  it("renders no badge when the node has no findings (non-regression)", () => {
    render(<GraphNode data={data()} />);
    expect(screen.queryByTestId("finding-badge")).toBeNull();
  });
});
