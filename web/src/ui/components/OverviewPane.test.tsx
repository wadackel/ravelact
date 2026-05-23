import type { ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

// Pass-through stub for ResizableRightPane so existing role / label
// queries on the inner <aside> keep matching without re-resizable's
// DOM in jsdom.
vi.mock("./ResizableRightPane.tsx", () => ({
  ResizableRightPane: ({ children }: { children: ReactNode }) => <>{children}</>,
}));

import { OverviewPane, type OverviewPaneProps } from "./OverviewPane.tsx";
import type { TriggerSummary } from "../../lib/types.ts";

function renderOverview(overrides: Partial<OverviewPaneProps> = {}) {
  const props: OverviewPaneProps = {
    triggers: null,
    selectedEvent: null,
    onSelectEvent: () => {},
    width: 360,
    onWidthChange: () => {},
    ...overrides,
  };
  return render(<OverviewPane {...props} />);
}

const SAMPLE: TriggerSummary[] = [
  {
    event: "push",
    entry_workflows: 3,
    declarations: 3,
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
];

describe("OverviewPane", () => {
  afterEach(() => cleanup());

  it("shows a loading status when triggers is null", () => {
    renderOverview({ triggers: null });
    expect(screen.getByRole("status")).toHaveTextContent("Loading");
  });

  it("shows an empty message when triggers is an empty array", () => {
    renderOverview({ triggers: [] });
    expect(screen.getByText("No events declared")).toBeInTheDocument();
  });

  it("renders one button per event with the entry-workflows count", () => {
    renderOverview({ triggers: SAMPLE });
    const options = screen.getAllByRole("button", {
      name: /^(push|pull_request)/,
    });
    expect(options).toHaveLength(2);
    expect(options[0]).toHaveTextContent("push");
    expect(options[0]).toHaveTextContent("3");
    expect(options[1]).toHaveTextContent("pull_request");
    expect(options[1]).toHaveTextContent("1");
  });

  it("clicking an event button invokes onSelectEvent with that event", async () => {
    const user = userEvent.setup();
    const onSelectEvent = vi.fn();
    renderOverview({ triggers: SAMPLE, onSelectEvent });
    await user.click(screen.getByRole("button", { name: /^push/ }));
    expect(onSelectEvent).toHaveBeenCalledWith("push");
  });

  it("clicking the SELECTED event toggles it off (null)", async () => {
    const user = userEvent.setup();
    const onSelectEvent = vi.fn();
    renderOverview({ triggers: SAMPLE, selectedEvent: "push", onSelectEvent });
    await user.click(screen.getByRole("button", { name: /^push/ }));
    expect(onSelectEvent).toHaveBeenCalledWith(null);
  });

  it("marks the selected event with aria-pressed and exposes Clear", async () => {
    const user = userEvent.setup();
    const onSelectEvent = vi.fn();
    renderOverview({ triggers: SAMPLE, selectedEvent: "push", onSelectEvent });
    expect(screen.getByRole("button", { name: /^push/ })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: /^pull_request/ })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    await user.click(screen.getByRole("button", { name: "Clear event selection" }));
    expect(onSelectEvent).toHaveBeenCalledWith(null);
  });

  it("Escape inside the pane clears the selection when one is active", async () => {
    const user = userEvent.setup();
    const onSelectEvent = vi.fn();
    renderOverview({ triggers: SAMPLE, selectedEvent: "push", onSelectEvent });
    // Focus an event button inside the pane so the keydown lands on
    // the aside subtree, then dispatch Escape.
    await user.click(screen.getByRole("button", { name: /^push/ }));
    onSelectEvent.mockClear();
    await user.keyboard("{Escape}");
    expect(onSelectEvent).toHaveBeenCalledWith(null);
  });
});
