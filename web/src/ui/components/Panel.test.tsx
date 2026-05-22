import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

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
import type { NodeKind, RepoInfo } from "../../lib/types.ts";
import { Panel, type Tab } from "./Panel.tsx";

// Mirrors App.tsx's wiring: owns the active tab state and keys the
// inner Panel on `openFor.id` so a per-node remount still resets the
// data slices while the tab survives.
function ControlledPanel(props: {
  initialTab?: Tab;
  openFor: { id: string; kind: NodeKind };
  onClose: () => void;
  repoInfo: RepoInfo | null;
}) {
  const [tab, setTab] = useState<Tab>(props.initialTab ?? "details");
  return (
    <Panel
      key={props.openFor.id}
      openFor={props.openFor}
      onClose={props.onClose}
      repoInfo={props.repoInfo}
      tab={tab}
      onTabChange={setTab}
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
