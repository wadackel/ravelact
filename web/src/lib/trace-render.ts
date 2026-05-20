import type { TraceJsonNode } from "./types.ts";

/**
 * Render a `TraceJsonNode` tree as a plain-text indented string. The result
 * is rendered through React (e.g. inside `<pre>`), so React's built-in JSX
 * escaping handles any potentially unsafe characters from the input.
 *
 * Splitting this out of `Panel.tsx` is justified by testability — the pure
 * function form lets `tests/trace-render.test.ts` exercise every variant of
 * the discriminated union directly, including XSS-shaped inputs.
 */
export function renderTraceTree(node: TraceJsonNode, depth = 0): string {
  const indent = "  ".repeat(depth);
  switch (node.kind) {
    case "workflow":
      return [
        `${indent}↳ wf:${node.id}`,
        ...node.children.map((c) => renderTraceTree(c, depth + 1)),
      ].join("\n");
    case "action":
      return [
        `${indent}↳ la:${node.id}`,
        ...node.children.map((c) => renderTraceTree(c, depth + 1)),
      ].join("\n");
    case "external-action": {
      const sub = node.subpath ? "/" + node.subpath : "";
      return `${indent}↳ ea:${node.owner}/${node.repo}${sub}@${node.gitref}`;
    }
    case "external-workflow":
      return `${indent}↳ ew:${node.owner}/${node.repo}/${node.path}@${node.gitref}`;
    case "docker":
      return `${indent}↳ dk:${node.image}`;
    case "annotated":
      return [
        `${indent}↳ [${node.verb}] ${node.label}${node.dangling ? " (dangling)" : ""}`,
        ...node.children.map((c) => renderTraceTree(c, depth + 1)),
      ].join("\n");
    case "cycle":
      return `${indent}↳ cycle: ${node.target} (${node.target_kind})`;
    case "guarded":
      return [`${indent}? if: ${node.if_expr}`, renderTraceTree(node.inner, depth + 1)].join("\n");
  }
}
