import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { FindingsFloat } from "./FindingsFloat.tsx";
import type { FindingFacets } from "../../lib/graph-filter.ts";
import type { FindingWithNode, TriggerSummary } from "../../lib/types.ts";

afterEach(cleanup);

const NO_FACETS: FindingFacets = { severities: null, sources: null, contexts: null };

function row(
  over: Partial<{
    ruleId: string;
    sourceSeverity: string;
    source: string;
    file: string;
    line: number;
    nodeId: string;
    nodeKind: string;
  }>,
): FindingWithNode {
  return {
    finding: {
      ruleId: over.ruleId ?? "template-injection",
      message: "msg",
      sourceSeverity: over.sourceSeverity ?? "high",
      graphPriority: "high",
      priorityReasons: [],
      file: over.file ?? ".github/workflows/ci.yaml",
      line: over.line ?? 42,
      source: over.source ?? "zizmor",
    },
    nodeId: over.nodeId ?? "wf:.github/workflows/ci.yaml",
    nodeKind: over.nodeKind ?? "workflow",
  } as unknown as FindingWithNode;
}

const TRIGGERS: TriggerSummary[] = [
  {
    event: "pull_request",
    entryWorkflows: 1,
    declarations: 1,
    typed: 0,
    filtered: 0,
    examples: [],
  } as unknown as TriggerSummary,
];

function baseProps() {
  return {
    hasFindings: true,
    findings: [
      row({}),
      row({ ruleId: "shellcheck/SC2086", source: "actionlint", sourceSeverity: "low" }),
    ],
    availableSources: ["actionlint", "zizmor"],
    facets: NO_FACETS,
    onChangeFacets: vi.fn(),
    onSelectFinding: vi.fn(),
    triggers: TRIGGERS,
    selectedEvent: null as string | null,
    onSelectEvent: vi.fn(),
  };
}

describe("FindingsFloat", () => {
  it("defaults to the Findings tab and lists the cross-cutting findings", () => {
    render(<FindingsFloat {...baseProps()} />);
    expect(screen.getByTestId("findings-float")).toBeTruthy();
    const rows = screen.getAllByTestId("float-finding-row");
    expect(rows).toHaveLength(2);
    // Per-rule source badges render inside the rows.
    const badges = screen.getAllByTestId("source-badge").map((b) => b.textContent);
    expect(badges).toContain("zizmor");
    expect(badges).toContain("actionlint");
  });

  it("toggles a severity facet through onChangeFacets", () => {
    const props = baseProps();
    render(<FindingsFloat {...props} />);
    // "High" severity chip (count present from fixture rows).
    fireEvent.click(screen.getByRole("button", { name: /High/ }));
    expect(props.onChangeFacets).toHaveBeenCalledWith(
      expect.objectContaining({ severities: expect.any(Set) }),
    );
  });

  it("toggles a context lens through onChangeFacets", () => {
    const props = baseProps();
    render(<FindingsFloat {...props} />);
    fireEvent.click(screen.getByRole("button", { name: "Reachable from risky" }));
    expect(props.onChangeFacets).toHaveBeenCalledWith(
      expect.objectContaining({ contexts: expect.any(Set) }),
    );
  });

  it("selects the node + kind when a finding row is clicked", () => {
    const props = baseProps();
    render(<FindingsFloat {...props} />);
    fireEvent.click(screen.getAllByTestId("float-finding-row")[0]!);
    expect(props.onSelectFinding).toHaveBeenCalledWith("wf:.github/workflows/ci.yaml", "workflow");
  });

  it("switches to the Events tab and toggles an event", () => {
    const props = baseProps();
    render(<FindingsFloat {...props} />);
    fireEvent.click(screen.getByRole("tab", { name: /events/i }));
    const events = screen.getByRole("list", { name: "Events" });
    fireEvent.click(within(events).getByRole("button", { name: /pull_request/ }));
    expect(props.onSelectEvent).toHaveBeenCalledWith("pull_request");
  });

  it("collapses the body when the chevron is toggled", () => {
    render(<FindingsFloat {...baseProps()} />);
    expect(screen.getAllByTestId("float-finding-row").length).toBeGreaterThan(0);
    fireEvent.click(screen.getByRole("button", { name: "Collapse panel" }));
    expect(screen.queryByTestId("float-finding-row")).toBeNull();
  });

  it("shows an empty state when findings are enabled but the list is empty", () => {
    const props = { ...baseProps(), findings: [] };
    render(<FindingsFloat {...props} />);
    expect(screen.getByText("No node-attached findings")).toBeTruthy();
  });

  it("clears the selected event on Escape within the panel", () => {
    const props = { ...baseProps(), selectedEvent: "pull_request" };
    render(<FindingsFloat {...props} />);
    fireEvent.keyDown(screen.getByTestId("findings-float"), { key: "Escape" });
    expect(props.onSelectEvent).toHaveBeenCalledWith(null);
  });
});
