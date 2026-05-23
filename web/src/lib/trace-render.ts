import type { TraceJsonNode } from "./types.ts";

/**
 * Render a `TraceJsonNode` tree as a plain-text indented string. The
 * result is rendered through React (e.g. inside `<pre>`), so React's
 * built-in JSX escaping handles any potentially unsafe characters
 * from the input.
 *
 * Splitting this out of `Panel.tsx` is justified by testability — the
 * pure function form lets `tests/trace-render.test.ts` exercise every
 * variant of the protobuf `oneof TraceJsonNode.node` directly,
 * including XSS-shaped inputs.
 */
export function renderTraceTree(node: TraceJsonNode, depth = 0): string {
  const indent = "  ".repeat(depth);
  const variant = node.node;
  if (variant === undefined || variant.case === undefined) {
    // Server always emits exactly one oneof arm. Render defensively
    // so a missing payload surfaces as a visible marker rather than
    // an empty fragment.
    return `${indent}↳ (empty)`;
  }
  switch (variant.case) {
    case "workflow":
      return [
        `${indent}↳ wf:${variant.value.id}`,
        ...variant.value.children.map((c) => renderTraceTree(c, depth + 1)),
      ].join("\n");
    case "action":
      return [
        `${indent}↳ la:${variant.value.id}`,
        ...variant.value.children.map((c) => renderTraceTree(c, depth + 1)),
      ].join("\n");
    case "externalAction": {
      const sub = variant.value.subpath ? "/" + variant.value.subpath : "";
      return `${indent}↳ ea:${variant.value.owner}/${variant.value.repo}${sub}@${variant.value.gitref}`;
    }
    case "externalWorkflow":
      return `${indent}↳ ew:${variant.value.owner}/${variant.value.repo}/${variant.value.path}@${variant.value.gitref}`;
    case "docker":
      return `${indent}↳ dk:${variant.value.image}`;
    case "annotated":
      return [
        `${indent}↳ [${variant.value.verb}] ${variant.value.label}${variant.value.dangling ? " (dangling)" : ""}`,
        ...variant.value.children.map((c) => renderTraceTree(c, depth + 1)),
      ].join("\n");
    case "cycle":
      return `${indent}↳ cycle: ${variant.value.target} (${variant.value.targetKind})`;
    case "guarded": {
      const inner = variant.value.inner;
      const innerRendered =
        inner === undefined
          ? `${"  ".repeat(depth + 1)}↳ (empty)`
          : renderTraceTree(inner, depth + 1);
      return [`${indent}? if: ${variant.value.ifExpr}`, innerRendered].join("\n");
    }
  }
}
