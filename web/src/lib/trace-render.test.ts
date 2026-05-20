import { describe, expect, it } from "vitest";
import { renderTraceTree } from "./trace-render.ts";
import type { TraceJsonNode } from "./types.ts";

describe("renderTraceTree — all TraceJsonNode variants", () => {
  it("workflow with nested action", () => {
    const tree: TraceJsonNode = {
      kind: "workflow",
      id: ".github/workflows/ci.yml",
      children: [{ kind: "action", id: "checkout", children: [] }],
    };
    expect(renderTraceTree(tree)).toBe("↳ wf:.github/workflows/ci.yml\n  ↳ la:checkout");
  });

  it("action leaf", () => {
    const tree: TraceJsonNode = { kind: "action", id: "x", children: [] };
    expect(renderTraceTree(tree)).toBe("↳ la:x");
  });

  it("external-action with subpath", () => {
    const tree: TraceJsonNode = {
      kind: "external-action",
      owner: "actions",
      repo: "checkout",
      subpath: "sub/dir",
      gitref: "v4",
    };
    expect(renderTraceTree(tree)).toBe("↳ ea:actions/checkout/sub/dir@v4");
  });

  it("external-action without subpath", () => {
    const tree: TraceJsonNode = {
      kind: "external-action",
      owner: "actions",
      repo: "checkout",
      gitref: "v4",
    };
    expect(renderTraceTree(tree)).toBe("↳ ea:actions/checkout@v4");
  });

  it("external-workflow", () => {
    const tree: TraceJsonNode = {
      kind: "external-workflow",
      owner: "x",
      repo: "y",
      path: ".github/workflows/r.yml",
      gitref: "main",
    };
    expect(renderTraceTree(tree)).toBe("↳ ew:x/y/.github/workflows/r.yml@main");
  });

  it("docker leaf", () => {
    const tree: TraceJsonNode = { kind: "docker", image: "alpine:3.18" };
    expect(renderTraceTree(tree)).toBe("↳ dk:alpine:3.18");
  });

  it("annotated with dangling marker", () => {
    const tree: TraceJsonNode = {
      kind: "annotated",
      verb: "Dispatches",
      dangling: true,
      label: "missing-target",
      children: [],
    };
    expect(renderTraceTree(tree)).toBe("↳ [Dispatches] missing-target (dangling)");
  });

  it("annotated without dangling, with children", () => {
    const tree: TraceJsonNode = {
      kind: "annotated",
      verb: "Triggers",
      dangling: false,
      label: "target",
      children: [{ kind: "workflow", id: "wf", children: [] }],
    };
    expect(renderTraceTree(tree)).toBe("↳ [Triggers] target\n  ↳ wf:wf");
  });

  it("cycle leaf", () => {
    const tree: TraceJsonNode = {
      kind: "cycle",
      target_kind: "workflow",
      target: "x",
    };
    expect(renderTraceTree(tree)).toBe("↳ cycle: x (workflow)");
  });

  it("guarded wraps inner", () => {
    const tree: TraceJsonNode = {
      kind: "guarded",
      if_expr: "github.event_name == 'push'",
      inner: { kind: "workflow", id: "wf", children: [] },
    };
    expect(renderTraceTree(tree)).toBe("? if: github.event_name == 'push'\n  ↳ wf:wf");
  });

  it("preserves XSS-shaped input verbatim (consumer escapes via JSX)", () => {
    const payload = "<script>alert(1)</script>";
    const tree: TraceJsonNode = { kind: "docker", image: payload };
    // renderTraceTree must NOT pre-encode the payload. The string is
    // injected through React JSX (<pre>{string}</pre>), which performs the
    // HTML escape. Pre-encoding here would cause double-escaping when the
    // value is displayed. This test pins that contract.
    expect(renderTraceTree(tree)).toBe(`↳ dk:${payload}`);
  });

  it("nests deeply with cumulative indent", () => {
    const tree: TraceJsonNode = {
      kind: "workflow",
      id: "a",
      children: [
        {
          kind: "workflow",
          id: "b",
          children: [{ kind: "action", id: "c", children: [] }],
        },
      ],
    };
    expect(renderTraceTree(tree)).toBe("↳ wf:a\n  ↳ wf:b\n    ↳ la:c");
  });
});
