import { describe, expect, it } from "vitest";
import { renderTraceTree } from "./trace-render.ts";
import type { TraceJsonNode } from "./types.ts";

// Cheap factory: bypasses the protobuf-es `$typeName` discriminator
// check so test fixtures can stay concise. The unit under test only
// reads the `node` oneof, so the missing tag has no runtime effect.
function tj(node: TraceJsonNode["node"]): TraceJsonNode {
  return { node } as TraceJsonNode;
}

describe("renderTraceTree — all TraceJsonNode variants", () => {
  it("workflow with nested action", () => {
    const tree = tj({
      case: "workflow",
      value: {
        id: ".github/workflows/ci.yml",
        children: [
          tj({
            case: "action",
            value: { id: "checkout", children: [] } as never,
          }),
        ],
      } as never,
    });
    expect(renderTraceTree(tree)).toBe("↳ wf:.github/workflows/ci.yml\n  ↳ la:checkout");
  });

  it("action leaf", () => {
    const tree = tj({ case: "action", value: { id: "x", children: [] } as never });
    expect(renderTraceTree(tree)).toBe("↳ la:x");
  });

  it("external-action with subpath", () => {
    const tree = tj({
      case: "externalAction",
      value: {
        owner: "actions",
        repo: "checkout",
        subpath: "sub/dir",
        gitref: "v4",
      } as never,
    });
    expect(renderTraceTree(tree)).toBe("↳ ea:actions/checkout/sub/dir@v4");
  });

  it("external-action without subpath", () => {
    const tree = tj({
      case: "externalAction",
      value: {
        owner: "actions",
        repo: "checkout",
        gitref: "v4",
      } as never,
    });
    expect(renderTraceTree(tree)).toBe("↳ ea:actions/checkout@v4");
  });

  it("external-workflow", () => {
    const tree = tj({
      case: "externalWorkflow",
      value: {
        owner: "x",
        repo: "y",
        path: ".github/workflows/r.yml",
        gitref: "main",
      } as never,
    });
    expect(renderTraceTree(tree)).toBe("↳ ew:x/y/.github/workflows/r.yml@main");
  });

  it("docker leaf", () => {
    const tree = tj({ case: "docker", value: { image: "alpine:3.18" } as never });
    expect(renderTraceTree(tree)).toBe("↳ dk:alpine:3.18");
  });

  it("annotated with dangling marker", () => {
    const tree = tj({
      case: "annotated",
      value: {
        verb: "Dispatches",
        dangling: true,
        label: "missing-target",
        children: [],
      } as never,
    });
    expect(renderTraceTree(tree)).toBe("↳ [Dispatches] missing-target (dangling)");
  });

  it("annotated without dangling, with children", () => {
    const tree = tj({
      case: "annotated",
      value: {
        verb: "Triggers",
        dangling: false,
        label: "target",
        children: [tj({ case: "workflow", value: { id: "wf", children: [] } as never })],
      } as never,
    });
    expect(renderTraceTree(tree)).toBe("↳ [Triggers] target\n  ↳ wf:wf");
  });

  it("cycle leaf", () => {
    const tree = tj({
      case: "cycle",
      value: { targetKind: "workflow", target: "x" } as never,
    });
    expect(renderTraceTree(tree)).toBe("↳ cycle: x (workflow)");
  });

  it("guarded wraps inner", () => {
    const tree = tj({
      case: "guarded",
      value: {
        ifExpr: "github.event_name == 'push'",
        inner: tj({ case: "workflow", value: { id: "wf", children: [] } as never }),
      } as never,
    });
    expect(renderTraceTree(tree)).toBe("? if: github.event_name == 'push'\n  ↳ wf:wf");
  });

  it("preserves XSS-shaped input verbatim (consumer escapes via JSX)", () => {
    const payload = "<script>alert(1)</script>";
    const tree = tj({ case: "docker", value: { image: payload } as never });
    // renderTraceTree must NOT pre-encode the payload. The string is
    // injected through React JSX (<pre>{string}</pre>), which performs the
    // HTML escape. Pre-encoding here would cause double-escaping when the
    // value is displayed. This test pins that contract.
    expect(renderTraceTree(tree)).toBe(`↳ dk:${payload}`);
  });

  it("nests deeply with cumulative indent", () => {
    const tree = tj({
      case: "workflow",
      value: {
        id: "a",
        children: [
          tj({
            case: "workflow",
            value: {
              id: "b",
              children: [tj({ case: "action", value: { id: "c", children: [] } as never })],
            } as never,
          }),
        ],
      } as never,
    });
    expect(renderTraceTree(tree)).toBe("↳ wf:a\n  ↳ wf:b\n    ↳ la:c");
  });
});
