import { describe, expect, it } from "vitest";
import { computeLayout, computeLayoutSync } from "./graph-layout.ts";
import type { GraphPayload } from "./types.ts";

// vitest runs in jsdom which does not provide `globalThis.Worker`, so
// `computeLayout` exercises the synchronous fallback branch here. The
// real Worker round-trip is observed indirectly via the e2e suite
// running against a built bundle.
const TINY_PAYLOAD = {
  nodes: [
    { data: { id: "wf:a", label: "A", kind: "workflow" } },
    { data: { id: "wf:b", label: "B", kind: "workflow" } },
    { data: { id: "wf:c", label: "C", kind: "workflow" } },
  ],
  edges: [
    { data: { id: "e1", source: "wf:a", target: "wf:b", kind: "calls-workflow" } },
    { data: { id: "e2", source: "wf:b", target: "wf:c", kind: "calls-workflow" } },
  ],
} as unknown as GraphPayload;

const DANGLING_PAYLOAD = {
  nodes: [{ data: { id: "wf:a", label: "A", kind: "workflow" } }],
  edges: [{ data: { id: "ex", source: "wf:a", target: "wf:missing", kind: "calls-workflow" } }],
} as unknown as GraphPayload;

// One edge flagged on_dangerous_path, one not — exercises both markerEnd
// color branches (TINY_PAYLOAD edges carry no flag → blue branch only).
const DANGER_PAYLOAD = {
  nodes: [
    { data: { id: "wf:a", label: "A", kind: "workflow" } },
    { data: { id: "wf:b", label: "B", kind: "workflow" } },
  ],
  edges: [
    {
      data: {
        id: "e-danger",
        source: "wf:a",
        target: "wf:b",
        kind: "calls-workflow",
        onDangerousPath: true,
      },
    },
  ],
} as unknown as GraphPayload;

describe("computeLayoutSync", () => {
  it("returns a node + edge graph with positions assigned by dagre", () => {
    const result = computeLayoutSync(TINY_PAYLOAD);
    expect(result.nodes).toHaveLength(3);
    expect(result.edges).toHaveLength(2);
    for (const n of result.nodes) {
      expect(typeof n.position.x).toBe("number");
      expect(typeof n.position.y).toBe("number");
      expect(Number.isFinite(n.position.x)).toBe(true);
      expect(Number.isFinite(n.position.y)).toBe(true);
      expect(n.type).toBe("card");
      expect(n.data.faded).toBe(false);
    }
    // LR layout with two edges: positions should monotonically increase
    // along the x axis in topological order. Locks in the dagre rankdir
    // contract — if a future maintainer flips rankdir this fails loudly.
    const xs = result.nodes
      .map((n) => ({ id: n.id, x: n.position.x }))
      .sort((a, b) => a.x - b.x)
      .map((entry) => entry.id);
    expect(xs).toEqual(["wf:a", "wf:b", "wf:c"]);
  });

  it("preserves edge ids and adds the styled arrow marker", () => {
    const result = computeLayoutSync(TINY_PAYLOAD);
    expect(result.edges.map((e) => e.id).sort()).toEqual(["e1", "e2"]);
    for (const e of result.edges) {
      expect(e.type).toBe("default");
      expect(e.markerEnd).toMatchObject({ type: "arrowclosed" });
    }
  });

  it("colors the arrow marker blue for normal edges", () => {
    const result = computeLayoutSync(TINY_PAYLOAD);
    for (const e of result.edges) {
      expect(e.markerEnd).toMatchObject({ type: "arrowclosed", color: "#3b82f6" });
    }
  });

  it("colors the arrow marker red for dangerous-path edges", () => {
    const result = computeLayoutSync(DANGER_PAYLOAD);
    expect(result.edges).toHaveLength(1);
    expect(result.edges[0]!.markerEnd).toMatchObject({
      type: "arrowclosed",
      color: "#dc3545",
    });
    expect(result.edges[0]!.data?.onDangerousPath).toBe(true);
  });

  it("throws when an edge references a non-existent node id", () => {
    expect(() => computeLayoutSync(DANGLING_PAYLOAD)).toThrowError(
      /references missing target wf:missing/,
    );
  });
});

describe("computeLayout (jsdom — Worker undefined fallback)", () => {
  it("resolves to the same shape as computeLayoutSync for a valid payload", async () => {
    const [sync, async] = [computeLayoutSync(TINY_PAYLOAD), await computeLayout(TINY_PAYLOAD)];
    expect(async.nodes.map((n) => n.id).sort()).toEqual(sync.nodes.map((n) => n.id).sort());
    expect(async.edges.map((e) => e.id).sort()).toEqual(sync.edges.map((e) => e.id).sort());
  });

  it("rejects with the same error that computeLayoutSync would throw", async () => {
    await expect(computeLayout(DANGLING_PAYLOAD)).rejects.toThrowError(
      /references missing target wf:missing/,
    );
  });
});
