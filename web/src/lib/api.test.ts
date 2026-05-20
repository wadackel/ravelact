import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  fetchEventImpact,
  fetchGraph,
  fetchImpact,
  fetchNode,
  fetchSearch,
  fetchTrace,
  fetchTriggers,
} from "./api.ts";

function mockFetch(response: Response): ReturnType<typeof vi.fn> {
  const spy = vi.fn().mockResolvedValue(response);
  vi.stubGlobal("fetch", spy);
  return spy;
}

describe("lib/api", () => {
  beforeEach(() => {
    vi.unstubAllGlobals();
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("fetchNode encodes both kind and id (slashes percent-encoded)", async () => {
    const spy = mockFetch(
      new Response(
        JSON.stringify({
          id: "wf:.github/workflows/x.yml",
          kind: "workflow",
          label: "x",
          file: "",
          summary: "",
          entry_triggers: [],
          refs_in: [],
          refs_out: [],
        }),
        { status: 200 },
      ),
    );
    await fetchNode("workflow", ".github/workflows/x.yml");
    expect(spy).toHaveBeenCalledWith("/api/node?kind=workflow&id=.github%2Fworkflows%2Fx.yml");
  });

  it("fetchImpact encodes id in query string", async () => {
    const spy = mockFetch(
      new Response(JSON.stringify({ workflows: [], actions: [], unknowns: [] }), { status: 200 }),
    );
    await fetchImpact(".github/workflows/x.yml");
    expect(spy).toHaveBeenCalledWith("/api/impact?id=.github%2Fworkflows%2Fx.yml");
  });

  it("fetchTrace encodes id in query string", async () => {
    const spy = mockFetch(
      new Response(
        JSON.stringify({
          tree: { kind: "workflow", id: "x", children: [] },
          event_used: "push",
        }),
        { status: 200 },
      ),
    );
    await fetchTrace(".github/workflows/wf.yml");
    expect(spy).toHaveBeenCalledWith("/api/trace?id=.github%2Fworkflows%2Fwf.yml");
  });

  it("returns null when the server responds 404", async () => {
    mockFetch(new Response("", { status: 404 }));
    expect(await fetchNode("workflow", "missing")).toBeNull();
    mockFetch(new Response("", { status: 404 }));
    expect(await fetchImpact("missing")).toBeNull();
    mockFetch(new Response("", { status: 404 }));
    expect(await fetchTrace("missing")).toBeNull();
    mockFetch(new Response("", { status: 404 }));
    expect(await fetchTriggers()).toBeNull();
  });

  it("throws on 5xx for query-string endpoints", async () => {
    mockFetch(new Response("", { status: 500 }));
    await expect(fetchNode("workflow", "x")).rejects.toThrow("500");
  });

  it("fetchGraph throws on any non-OK status (not nullable)", async () => {
    mockFetch(new Response("", { status: 500 }));
    await expect(fetchGraph()).rejects.toThrow("/api/graph 500");
  });

  it("fetchSearch encodes whitespace + forwards AbortSignal", async () => {
    const spy = mockFetch(
      new Response(JSON.stringify({ matches: [], truncated: false, total: 0 }), { status: 200 }),
    );
    const controller = new AbortController();
    await fetchSearch("a b", controller.signal);
    expect(spy).toHaveBeenCalledWith(
      "/api/search?q=a%20b",
      expect.objectContaining({ signal: controller.signal }),
    );
  });

  it("fetchSearch throws on non-OK status", async () => {
    mockFetch(new Response("", { status: 500 }));
    await expect(fetchSearch("x")).rejects.toThrow("/api/search 500");
  });

  it("fetchEventImpact encodes event + forwards AbortSignal", async () => {
    const spy = mockFetch(
      new Response(JSON.stringify({ event: "push", entry_workflows: [], node_ids: [] }), {
        status: 200,
      }),
    );
    const controller = new AbortController();
    await fetchEventImpact("push", controller.signal);
    expect(spy).toHaveBeenCalledWith(
      "/api/event-impact?event=push",
      expect.objectContaining({ signal: controller.signal }),
    );
  });

  it("fetchEventImpact throws on non-OK status", async () => {
    mockFetch(new Response("", { status: 500 }));
    await expect(fetchEventImpact("push")).rejects.toThrow("/api/event-impact 500");
  });

  it("fetchTriggers returns parsed body on 200", async () => {
    const body = {
      rows: [
        {
          event: "push",
          entry_workflows: 3,
          declarations: 3,
          typed: 0,
          filtered: 0,
          examples: [],
        },
      ],
    };
    mockFetch(new Response(JSON.stringify(body), { status: 200 }));
    const r = await fetchTriggers();
    expect(r?.rows[0]?.event).toBe("push");
    expect(r?.rows[0]?.entry_workflows).toBe(3);
  });
});
