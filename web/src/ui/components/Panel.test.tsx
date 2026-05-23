import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";

// Mock the api module BEFORE importing Panel so its module-level
// `import` resolves to the spies. `vi.mock` is hoisted in vitest.
vi.mock("../../lib/api.ts", () => ({
  fetchNode: vi.fn(),
  fetchImpact: vi.fn(),
  fetchTrace: vi.fn(),
  // unused by Panel but kept to satisfy other imports in the same module
  fetchGraph: vi.fn(),
  fetchTriggers: vi.fn(),
  fetchSearch: vi.fn(),
  fetchEventImpact: vi.fn(),
}));

import * as api from "../../lib/api.ts";
import type { IfCondition, NodeKind, RepoInfo } from "../../lib/types.ts";
import { Panel, type Tab } from "./Panel.tsx";

// Mirrors App.tsx's wiring: owns the active tab + event-selection
// state and keys the inner Panel on `openFor.id` so a per-node
// remount still resets the data slices while the tab survives.
function ControlledPanel(props: {
  initialTab?: Tab;
  openFor: { id: string; kind: NodeKind };
  onClose: () => void;
  repoInfo: RepoInfo | null;
  initialSelectedEvent?: string | null;
  onSelectEvent?: (event: string | null) => void;
}) {
  const [tab, setTab] = useState<Tab>(props.initialTab ?? "details");
  const [selectedEvent, setSelectedEvent] = useState<string | null>(
    props.initialSelectedEvent ?? null,
  );
  const handleSelectEvent = (event: string | null) => {
    setSelectedEvent(event);
    props.onSelectEvent?.(event);
  };
  return (
    <Panel
      key={props.openFor.id}
      openFor={props.openFor}
      onClose={props.onClose}
      repoInfo={props.repoInfo}
      tab={tab}
      onTabChange={setTab}
      selectedEvent={selectedEvent}
      onSelectEvent={handleSelectEvent}
    />
  );
}

const REPO: RepoInfo = {
  host: "github.com",
  owner: "wadackel",
  repo: "ravelact",
  ref: "main",
};

function nodeResponse(id: string) {
  return {
    id,
    kind: "workflow" as const,
    label: id,
    file: ".github/workflows/x.yaml",
    summary: "1 job(s), 1 trigger(s)",
    entry_triggers: ["push"],
    refs_in: [],
    refs_out: [],
    if_conditions: [],
  };
}

describe("Panel — fetch + cacheRef invariants", () => {
  beforeEach(() => {
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValue(nodeResponse("wf:x"));
    (api.fetchImpact as ReturnType<typeof vi.fn>).mockResolvedValue({
      workflows: [],
      actions: [],
      unknowns: [],
    });
    (api.fetchTrace as ReturnType<typeof vi.fn>).mockResolvedValue({
      tree: { kind: "workflow", id: "x", children: [] },
      event_used: "push",
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("fires fetchNode once on initial mount with a non-null openFor", async () => {
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    await waitFor(() => {
      expect(api.fetchNode).toHaveBeenCalledTimes(1);
    });
    expect(api.fetchNode).toHaveBeenCalledWith("workflow", "x");
  });

  it("clicking the Impact tab fires fetchImpact once", async () => {
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    await waitFor(() => expect(api.fetchNode).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("tab", { name: "Impact" }));
    await waitFor(() => {
      expect(api.fetchImpact).toHaveBeenCalledTimes(1);
    });
    expect(api.fetchImpact).toHaveBeenCalledWith("x");
  });

  it("clicking the same tab twice does NOT re-fetch (cacheRef hit)", async () => {
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Trace" }));
    await waitFor(() => expect(api.fetchTrace).toHaveBeenCalledTimes(1));
    // Click another tab and back to Trace — should hit the cache.
    fireEvent.click(screen.getByRole("tab", { name: "Details" }));
    fireEvent.click(screen.getByRole("tab", { name: "Trace" }));
    // Give the effect a tick to maybe (incorrectly) refire.
    await new Promise((r) => setTimeout(r, 30));
    expect(api.fetchTrace).toHaveBeenCalledTimes(1);
  });

  it("controlled tab prop renders the matching section without a click", async () => {
    render(
      <ControlledPanel
        initialTab="triggers"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    // Triggers tab should be active without simulating a click.
    const triggersTab = screen.getByRole("tab", { name: "Triggers" });
    expect(triggersTab).toHaveAttribute("aria-selected", "true");
    // Details fetch is shared by Details + Triggers, so wait for it,
    // then assert the Triggers content (the "push" chip from the mock).
    await waitFor(() => expect(api.fetchNode).toHaveBeenCalledTimes(1));
    expect(await screen.findByText("push")).toBeVisible();
  });

  it("renders Open-in-GitHub link in Details for a workflow node when repoInfo is set", async () => {
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:.github/workflows/ci.yaml", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={REPO}
      />,
    );
    // The link lives inside Details and is rendered only after the
    // fetchNode response resolves (Details shows Loading… until then).
    const link = await screen.findByRole("link", { name: "Open in GitHub" });
    expect(link).toHaveAttribute(
      "href",
      "https://github.com/wadackel/ravelact/blob/main/.github/workflows/ci.yaml",
    );
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noopener noreferrer");
  });

  it("hides Open-in-GitHub link for a workflow node when repoInfo is null", async () => {
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:.github/workflows/ci.yaml", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    // Wait for Details to leave the Loading state so we are asserting on
    // the populated tab rather than the placeholder.
    await screen.findByText(".github/workflows/x.yaml");
    expect(screen.queryByRole("link", { name: "Open in GitHub" })).toBeNull();
  });

  it("renders Open-in-GitHub link for an external-action regardless of repoInfo", async () => {
    // The default beforeEach mock returns a `workflow` NodeResponse for
    // every fetchNode call. Override here so Details renders for the
    // external-action kind (the union type requires `external-action`
    // when the node id starts with `ea:`).
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      id: "ea:actions/checkout@v4",
      kind: "external-action" as const,
      label: "actions/checkout@v4",
      file: "",
      summary: "actions/checkout@v4",
      entry_triggers: [],
      refs_in: [],
      refs_out: [],
      if_conditions: [],
    });
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "ea:actions/checkout@v4", kind: "external-action" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    const link = await screen.findByRole("link", { name: "Open in GitHub" });
    expect(link).toHaveAttribute("href", "https://github.com/actions/checkout/tree/v4");
  });

  it("hides Open-in-GitHub link for a docker node (404 Details)", async () => {
    // fetchNode 404s for docker — Details renders "Not found" and the
    // link field is absent.
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce(null);
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "dk:alpine:3.20", kind: "docker" }}
        onClose={() => {}}
        repoInfo={REPO}
      />,
    );
    await screen.findByText("Not found");
    expect(screen.queryByRole("link", { name: "Open in GitHub" })).toBeNull();
  });

  it("Triggers tab chip is a button with aria-pressed=false when no event is selected", async () => {
    render(
      <ControlledPanel
        initialTab="triggers"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    const chip = await screen.findByRole("button", { name: "push" });
    expect(chip).toHaveAttribute("aria-pressed", "false");
  });

  it("clicking a Triggers tab chip fires onSelectEvent with the event", async () => {
    const onSelectEvent = vi.fn();
    render(
      <ControlledPanel
        initialTab="triggers"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
        onSelectEvent={onSelectEvent}
      />,
    );
    const chip = await screen.findByRole("button", { name: "push" });
    fireEvent.click(chip);
    expect(onSelectEvent).toHaveBeenCalledWith("push");
  });

  it("re-clicking the active Triggers tab chip fires onSelectEvent(null) (toggle off)", async () => {
    const onSelectEvent = vi.fn();
    render(
      <ControlledPanel
        initialTab="triggers"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
        initialSelectedEvent="push"
        onSelectEvent={onSelectEvent}
      />,
    );
    const chip = await screen.findByRole("button", { name: "push" });
    expect(chip).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(chip);
    expect(onSelectEvent).toHaveBeenCalledWith(null);
  });

  it("active Triggers tab chip carries the aria-pressed accent style tokens (drift guard)", async () => {
    render(
      <ControlledPanel
        initialTab="triggers"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
        initialSelectedEvent="push"
      />,
    );
    const chip = await screen.findByRole("button", { name: "push" });
    // The active-state styling pivots on `aria-pressed:` variants. If
    // a refactor drops the accent tokens the visual cue regresses
    // silently — this guard catches that without depending on a real
    // CSS engine.
    expect(chip.className).toContain("aria-pressed:bg-");
    expect(chip.className).toContain("aria-pressed:border-accent");
    expect(chip.className).toContain("aria-pressed:text-accent");
  });

  it("Trace tab 'Event used' chip is a button and drives onSelectEvent on click", async () => {
    const onSelectEvent = vi.fn();
    render(
      <ControlledPanel
        initialTab="trace"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
        onSelectEvent={onSelectEvent}
      />,
    );
    // The trace mock at the top of the suite returns event_used: "push".
    const chip = await screen.findByRole("button", { name: "push" });
    expect(chip).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(chip);
    expect(onSelectEvent).toHaveBeenCalledWith("push");
  });

  it("Trace tab 'Event used' chip reflects selectedEvent and toggles off on re-click", async () => {
    const onSelectEvent = vi.fn();
    render(
      <ControlledPanel
        initialTab="trace"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
        initialSelectedEvent="push"
        onSelectEvent={onSelectEvent}
      />,
    );
    const chip = await screen.findByRole("button", { name: "push" });
    expect(chip).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(chip);
    expect(onSelectEvent).toHaveBeenCalledWith(null);
  });

  it("changing openFor to a new node invalidates the cache and re-fetches", async () => {
    // Mirror App.tsx's wiring via ControlledPanel: it keys the inner
    // <Panel> on `openFor.id` so each id change forces a remount, and
    // each Panel instance starts with fresh `state` and re-fetches.
    const { rerender } = render(
      <ControlledPanel
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    await waitFor(() => expect(api.fetchNode).toHaveBeenCalledTimes(1));

    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce(nodeResponse("wf:y"));
    rerender(
      <ControlledPanel
        openFor={{ id: "wf:y", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    await waitFor(() => {
      expect(api.fetchNode).toHaveBeenCalledTimes(2);
    });
    expect(api.fetchNode).toHaveBeenLastCalledWith("workflow", "y");
  });
});

// The Copy button is exercised separately from the fetch/cache suite above so
// the per-test clipboard mock and fake-timer lifecycle stay contained — those
// affect microtask ordering and would complicate the existing assertions.
describe("Panel — Copy button", () => {
  let writeText: ReturnType<typeof vi.fn>;
  let originalClipboard: PropertyDescriptor | undefined;

  beforeEach(() => {
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValue(nodeResponse("wf:x"));
    writeText = vi.fn();
    originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
      writable: true,
    });
  });

  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
    vi.useRealTimers();
    if (originalClipboard) {
      Object.defineProperty(navigator, "clipboard", originalClipboard);
    } else {
      // jsdom may not pre-define `navigator.clipboard`; in that case drop
      // the property we installed instead of leaving the stub behind.
      delete (navigator as { clipboard?: unknown }).clipboard;
    }
  });

  it("renders the Copy button alongside the File row", async () => {
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    await screen.findByText(".github/workflows/x.yaml");
    expect(screen.getByRole("button", { name: "Copy file path" })).toBeVisible();
  });

  it("writes the displayed file path on click and shows the Copied affordance", async () => {
    writeText.mockResolvedValue(undefined);
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    await screen.findByText(".github/workflows/x.yaml");

    fireEvent.click(screen.getByRole("button", { name: "Copy file path" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(".github/workflows/x.yaml"));
    // aria-live region announces success; sr-only but in the DOM.
    await waitFor(() => expect(screen.getByText("Copied")).toBeInTheDocument());

    // The feedback window is 1500 ms (COPY_FEEDBACK_MS); allow a small
    // cushion to absorb scheduler / jsdom jitter.
    await waitFor(() => expect(screen.queryByText("Copied")).not.toBeInTheDocument(), {
      timeout: 3000,
    });
  });

  it("falls back to a Copy failed state when writeText rejects, without an error log", async () => {
    writeText.mockRejectedValue(new Error("denied"));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      render(
        <ControlledPanel
          initialTab="details"
          openFor={{ id: "wf:x", kind: "workflow" }}
          onClose={() => {}}
          repoInfo={null}
        />,
      );
      await screen.findByText(".github/workflows/x.yaml");

      fireEvent.click(screen.getByRole("button", { name: "Copy file path" }));
      await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
      await waitFor(() => expect(screen.getByText("Copy failed")).toBeInTheDocument());

      expect(warn).toHaveBeenCalled();
      expect(err).not.toHaveBeenCalled();

      await waitFor(() => expect(screen.queryByText("Copy failed")).not.toBeInTheDocument(), {
        timeout: 3000,
      });
    } finally {
      warn.mockRestore();
      err.mockRestore();
    }
  });
});

// ---------------------------------------------------------------------------
// Conditions surface (Details tab — `/api/node` `if_conditions` payload)
// ---------------------------------------------------------------------------

function nodeWithIfConditions(
  if_conditions: IfCondition[],
  overrides: Partial<{ id: string; kind: "workflow" | "local-action" | "external-action" }> = {},
) {
  return {
    id: overrides.id ?? "wf:x",
    kind: overrides.kind ?? ("workflow" as const),
    label: overrides.id ?? "wf:x",
    file: ".github/workflows/x.yaml",
    summary: "1 job(s), 1 trigger(s)",
    entry_triggers: ["push"],
    refs_in: [],
    refs_out: [],
    if_conditions,
  };
}

describe("Panel — Conditions surface", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("hides the Conditions surface when if_conditions is empty (workflow)", async () => {
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce(nodeWithIfConditions([]));
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    // Wait for Details to leave the loading state.
    await screen.findByText(".github/workflows/x.yaml");
    expect(screen.queryByText("Conditions")).toBeNull();
  });

  it("hides the Conditions surface for an external-action node (always empty)", async () => {
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      nodeWithIfConditions([], { id: "ea:actions/checkout@v4", kind: "external-action" }),
    );
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "ea:actions/checkout@v4", kind: "external-action" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    // Wait for Details to render (Label appears regardless of file/summary).
    await screen.findByText("ea:actions/checkout@v4");
    expect(screen.queryByText("Conditions")).toBeNull();
  });

  it("renders a job-level if condition with job id and expression", async () => {
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      nodeWithIfConditions([
        {
          scope: "job",
          job_id: "deploy",
          expression: "github.ref == 'refs/heads/main'",
        },
      ]),
    );
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    expect(await screen.findByText("Conditions")).toBeVisible();
    expect(screen.getByText("job deploy")).toBeVisible();
    expect(screen.getByText("github.ref == 'refs/heads/main'")).toBeVisible();
  });

  it("renders workflow job + step entries preserving source order with step context", async () => {
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      nodeWithIfConditions([
        {
          scope: "job",
          job_id: "build",
          expression: "EXPR_JOB",
        },
        {
          scope: "step",
          job_id: "build",
          step_index: 1,
          step_id: null,
          step_name: "Compile",
          expression: "EXPR_STEP",
        },
      ]),
    );
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    await screen.findByText("Conditions");
    // Source order: job entry first, step entry second. Walk the row list
    // in DOM order and read the prefix/expression text per row.
    const rows = screen.getAllByTestId("condition-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]!).getByText("job build")).toBeVisible();
    expect(within(rows[0]!).getByText("EXPR_JOB")).toBeVisible();
    expect(within(rows[1]!).getByText("step #1 (build / Compile)")).toBeVisible();
    expect(within(rows[1]!).getByText("EXPR_STEP")).toBeVisible();
  });

  it("renders a composite local-action step entry without job-id context", async () => {
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      nodeWithIfConditions(
        [
          {
            scope: "step",
            job_id: null,
            step_index: 1,
            step_id: "finalize",
            step_name: null,
            expression: "runner.os == 'Linux'",
          },
        ],
        { id: "la:.github/actions/foo", kind: "local-action" },
      ),
    );
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "la:.github/actions/foo", kind: "local-action" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    const row = await screen.findByTestId("condition-row");
    expect(within(row).getByText("step #1 (finalize)")).toBeVisible();
    expect(within(row).getByText("runner.os == 'Linux'")).toBeVisible();
    // job_id is null here — no "/ <job>" segment should appear. Scoping
    // the negative check to the condition row avoids false positives
    // from surrounding chrome that may legitimately mention other ids.
    expect(row.textContent ?? "").not.toContain(" / ");
  });

  it("preserves newlines in a multiline expression with whitespace-pre-wrap", async () => {
    const multiline = "github.event_name == 'push'\n&& startsWith(github.ref, 'refs/tags/')";
    (api.fetchNode as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      nodeWithIfConditions([
        {
          scope: "step",
          job_id: "release",
          step_index: 1,
          step_id: null,
          step_name: null,
          expression: multiline,
        },
      ]),
    );
    render(
      <ControlledPanel
        initialTab="details"
        openFor={{ id: "wf:x", kind: "workflow" }}
        onClose={() => {}}
        repoInfo={null}
      />,
    );
    // testing-library's default normalizer collapses whitespace, which
    // erases the `\n` we are asserting on. Read raw `textContent` off the
    // condition row directly and verify the `whitespace-pre-wrap`
    // utility is in place so visual line preservation does not silently
    // regress.
    const row = await screen.findByTestId("condition-row");
    const wrapper = row.querySelector<HTMLDivElement>("div");
    expect(wrapper).not.toBeNull();
    expect(wrapper?.className).toContain("whitespace-pre-wrap");
    expect(row.textContent ?? "").toContain("\n");
    expect(row.textContent ?? "").toContain(multiline);
  });
});
