import { describe, expect, it } from "vitest";
import { type EdgeRef, isVisible, reachableSet } from "./graph-filter.ts";

describe("reachableSet", () => {
  it("includes successors from a linear chain", () => {
    const edges: EdgeRef[] = [
      { source: "A", target: "B" },
      { source: "B", target: "C" },
    ];
    expect(reachableSet(edges, "A")).toEqual(new Set(["A", "B", "C"]));
  });

  it("includes predecessors AND successors from a middle node", () => {
    const edges: EdgeRef[] = [
      { source: "A", target: "B" },
      { source: "B", target: "C" },
    ];
    expect(reachableSet(edges, "B")).toEqual(new Set(["A", "B", "C"]));
  });

  it("walks predecessors of predecessors AND successors of successors", () => {
    // R → A → C, R → B (B is a sibling, not on either path from C).
    const edges: EdgeRef[] = [
      { source: "R", target: "A" },
      { source: "R", target: "B" },
      { source: "A", target: "C" },
    ];
    // From C: backward A,R reached; forward nothing. R's other successor
    // B is NOT visited — the walk does one forward pass from root and
    // one backward pass from root, not "bwd then fwd from each visited".
    expect(reachableSet(edges, "C")).toEqual(new Set(["C", "A", "R"]));
  });

  it("does NOT recurse fwd-of-predecessors (sibling B excluded)", () => {
    const edges: EdgeRef[] = [
      { source: "R", target: "A" },
      { source: "R", target: "B" },
    ];
    // From A: bwd reaches R; B is a successor of R, not of A. Excluded.
    const out = reachableSet(edges, "A");
    expect(out.has("R")).toBe(true);
    expect(out.has("B")).toBe(false);
  });

  it("returns just the root when isolated", () => {
    expect(reachableSet([], "solo")).toEqual(new Set(["solo"]));
  });
});

describe("isVisible — 3-filter OR composition", () => {
  const allNull = { matchedIds: null, analysisIds: null, reachable: null };

  it("all filters null → every id is visible", () => {
    expect(isVisible("a", allNull)).toBe(true);
    expect(isVisible("anything", allNull)).toBe(true);
  });

  it("matchedIds only → only members are visible", () => {
    const f = { ...allNull, matchedIds: new Set(["a"]) };
    expect(isVisible("a", f)).toBe(true);
    expect(isVisible("b", f)).toBe(false);
  });

  it("analysisIds only → only members are visible", () => {
    const f = { ...allNull, analysisIds: new Set(["a"]) };
    expect(isVisible("a", f)).toBe(true);
    expect(isVisible("b", f)).toBe(false);
  });

  it("reachable only → only members are visible", () => {
    const f = { ...allNull, reachable: new Set(["a"]) };
    expect(isVisible("a", f)).toBe(true);
    expect(isVisible("b", f)).toBe(false);
  });

  it("matchedIds ∪ analysisIds: union is visible", () => {
    const f = {
      ...allNull,
      matchedIds: new Set(["a"]),
      analysisIds: new Set(["b"]),
    };
    expect(isVisible("a", f)).toBe(true);
    expect(isVisible("b", f)).toBe(true);
    expect(isVisible("c", f)).toBe(false);
  });

  it("three active sets: id must be in at least one", () => {
    const f = {
      matchedIds: new Set(["a"]),
      analysisIds: new Set(["b"]),
      reachable: new Set(["c"]),
    };
    expect(isVisible("a", f)).toBe(true);
    expect(isVisible("b", f)).toBe(true);
    expect(isVisible("c", f)).toBe(true);
    expect(isVisible("d", f)).toBe(false);
  });

  it("empty Set (active filter with zero hits) excludes every id", () => {
    const f = { ...allNull, matchedIds: new Set<string>() };
    expect(isVisible("a", f)).toBe(false);
    expect(isVisible("b", f)).toBe(false);
  });
});
