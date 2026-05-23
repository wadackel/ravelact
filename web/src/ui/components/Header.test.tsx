import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Header } from "./Header.tsx";
import type { TriggerSummary } from "../../lib/types.ts";

const SAMPLE_TRIGGERS: TriggerSummary[] = [
  {
    event: "push",
    entryWorkflows: 3,
    declarations: 3,
    typed: 0,
    filtered: 0,
    examples: [],
  } as unknown as TriggerSummary,
];

function renderHeader(overrides: Partial<React.ComponentProps<typeof Header>> = {}) {
  const props = {
    nodeCount: 10,
    triggers: SAMPLE_TRIGGERS,
    searchQuery: "",
    onSearchChange: vi.fn(),
    onSearchEnter: vi.fn(),
    ...overrides,
  } as const;
  render(<Header {...props} />);
  return props;
}

describe("Header", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders 3-segment stats: events / entry workflows / nodes", () => {
    renderHeader();
    const stats = document.getElementById("stats");
    if (!stats) throw new Error("#stats not found");
    expect(stats).toHaveTextContent("1");
    expect(stats).toHaveTextContent("3");
    expect(stats).toHaveTextContent("10");
    expect(stats).toHaveTextContent(/events/);
    expect(stats).toHaveTextContent(/entry workflows/);
    expect(stats).toHaveTextContent(/nodes/);
  });

  it("Cmd+K focuses the search input", async () => {
    const user = userEvent.setup();
    renderHeader();
    const input = screen.getByLabelText("Search nodes, files, and triggers");
    expect(document.activeElement).not.toBe(input);
    // `{Meta>}` opens the modifier, `k` is the key, `{/Meta}` closes.
    await user.keyboard("{Meta>}k{/Meta}");
    expect(document.activeElement).toBe(input);
  });

  it("Ctrl+K focuses the search input (Linux/Windows path)", async () => {
    const user = userEvent.setup();
    renderHeader();
    const input = screen.getByLabelText("Search nodes, files, and triggers");
    await user.keyboard("{Control>}k{/Control}");
    expect(document.activeElement).toBe(input);
  });

  it("Escape on the input clears the value via onSearchChange and blurs", async () => {
    const user = userEvent.setup();
    const props = renderHeader({ searchQuery: "foo" });
    const input = screen.getByLabelText("Search nodes, files, and triggers") as HTMLInputElement;
    await user.click(input);
    expect(document.activeElement).toBe(input);
    await user.keyboard("{Escape}");
    expect(props.onSearchChange).toHaveBeenCalledWith("");
    expect(document.activeElement).not.toBe(input);
  });

  it("Enter on the input invokes onSearchEnter", async () => {
    const user = userEvent.setup();
    const props = renderHeader({ searchQuery: "ci" });
    const input = screen.getByLabelText("Search nodes, files, and triggers");
    await user.click(input);
    await user.keyboard("{Enter}");
    expect(props.onSearchEnter).toHaveBeenCalledTimes(1);
  });

  it("Fullscreen button calls requestFullscreen when not in fullscreen", async () => {
    const user = userEvent.setup();
    const requestSpy = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(document.documentElement, "requestFullscreen", {
      configurable: true,
      value: requestSpy,
    });
    renderHeader();
    await user.click(screen.getByRole("button", { name: "Toggle fullscreen" }));
    expect(requestSpy).toHaveBeenCalled();
  });
});
