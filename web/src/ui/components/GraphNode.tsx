import { memo } from "react";
import { Handle, Position } from "@xyflow/react";
import type { NodeKind } from "../../lib/types.ts";

// Per-node finding tally + OR-aggregated context flags. Present on a node
// only when it carries findings (total > 0); absent for a findings-free
// session, keeping the original card unchanged.
export type FindingOverlay = {
  counts: {
    error: number;
    high: number;
    medium: number;
    low: number;
    info: number;
    total: number;
  };
  // Distinct source tools producing findings on this node, e.g. ["zizmor"].
  sources: readonly string[];
  reachableFromRisky: boolean;
  isOrphan: boolean;
  hasWrite: boolean;
};

export type GraphNodeData = {
  name: string;
  subtitle: string;
  kind: NodeKind;
  faded: boolean;
  findings?: FindingOverlay;
};

// Most-severe tier present, mapping to a badge color bucket. Mirrors the
// M2 CLI `severity_style` ordering (error|high → high, medium → medium,
// otherwise low) so the browse badge and the `graph` Mermaid styling agree.
export type BadgeSeverity = "high" | "medium" | "low";

export function maxBadgeSeverity(c: FindingOverlay["counts"]): BadgeSeverity {
  if (c.error > 0 || c.high > 0) return "high";
  if (c.medium > 0) return "medium";
  return "low";
}

// Compact non-zero tally, severity order, e.g. `E1 H2 M1`.
export function compactCounts(c: FindingOverlay["counts"]): string {
  const parts: string[] = [];
  for (const [letter, n] of [
    ["E", c.error],
    ["H", c.high],
    ["M", c.medium],
    ["L", c.low],
    ["I", c.info],
  ] as const) {
    if (n > 0) parts.push(`${letter}${n}`);
  }
  return parts.join(" ");
}

const BASE_NODE =
  "inline-flex items-center gap-2 min-w-[140px] max-w-[240px] px-3 py-2 rounded-lg border bg-bg-elev cursor-pointer transition hover:shadow-[0_1px_3px_rgba(0,0,0,0.06),0_1px_2px_rgba(0,0,0,0.04)] data-[selected=true]:border-accent data-[selected=true]:shadow-[0_0_0_2px_color-mix(in_srgb,var(--color-accent)_18%,transparent)] data-[faded=true]:opacity-30";

// Per-kind background / border / dot color (via currentColor on the
// outer container) + name color (via descendant arbitrary variant on
// the `[data-rf-node-name]` child).
const COLOR_BY_KIND_NODE =
  "data-[kind=workflow]:bg-kind-workflow-fill data-[kind=workflow]:border-kind-workflow-border data-[kind=workflow]:text-kind-workflow-border [&[data-kind=workflow]_[data-rf-node-name]]:text-kind-workflow-text " +
  "data-[kind=local-action]:bg-kind-local-action-fill data-[kind=local-action]:border-kind-local-action-border data-[kind=local-action]:text-kind-local-action-border [&[data-kind=local-action]_[data-rf-node-name]]:text-kind-local-action-text " +
  "data-[kind=external-action]:bg-kind-external-action-fill data-[kind=external-action]:border-kind-external-action-border data-[kind=external-action]:text-kind-external-action-border [&[data-kind=external-action]_[data-rf-node-name]]:text-kind-external-action-text " +
  "data-[kind=external-workflow]:bg-kind-external-workflow-fill data-[kind=external-workflow]:border-kind-external-workflow-border data-[kind=external-workflow]:text-kind-external-workflow-border [&[data-kind=external-workflow]_[data-rf-node-name]]:text-kind-external-workflow-text " +
  "data-[kind=docker]:bg-kind-docker-fill data-[kind=docker]:border-kind-docker-border data-[kind=docker]:text-kind-docker-border [&[data-kind=docker]_[data-rf-node-name]]:text-kind-docker-text";

// `!` prefix forces important so ReactFlow's own .react-flow__handle
// defaults (which set width/height/border) are overridden.
const HANDLE_CLASS =
  "opacity-0 pointer-events-none !w-px !h-px min-w-[1px] min-h-[1px] !border-0 !bg-transparent";

// Severity badge colors keyed on the `data-severity` bucket. Hex mirrors the
// M2 `graph --highlight findings` Mermaid `severity_style` fills/strokes so
// the two surfaces stay visually consistent.
const BADGE_CLASS =
  "ml-1 shrink-0 inline-flex items-center rounded px-1.5 py-px text-[10px] font-semibold font-mono border " +
  "data-[severity=high]:bg-[#f8d7da] data-[severity=high]:text-[#842029] data-[severity=high]:border-[#dc3545] " +
  "data-[severity=medium]:bg-[#fff3cd] data-[severity=medium]:text-[#664d03] data-[severity=medium]:border-[#fd7e14] " +
  "data-[severity=low]:bg-[#e2e3e5] data-[severity=low]:text-[#41464b] data-[severity=low]:border-[#6c757d]";

function FindingBadge({ findings }: { findings: FindingOverlay }) {
  const severity = maxBadgeSeverity(findings.counts);
  const label = compactCounts(findings.counts);
  return (
    <span
      data-testid="finding-badge"
      data-severity={severity}
      className={BADGE_CLASS}
      title={`${findings.counts.total} finding(s): ${label}`}
      aria-label={`${findings.counts.total} findings: ${label}`}
    >
      {label}
    </span>
  );
}

function GraphNodeImpl({ data, selected }: { data: GraphNodeData; selected?: boolean }) {
  return (
    <div
      className={`${BASE_NODE} ${COLOR_BY_KIND_NODE}`}
      data-kind={data.kind}
      data-faded={data.faded ? "true" : undefined}
      data-selected={selected ? "true" : undefined}
    >
      <Handle
        type="target"
        position={Position.Left}
        isConnectable={false}
        className={HANDLE_CLASS}
      />
      <span className="w-[7px] h-[7px] rounded-full flex-shrink-0 bg-current" aria-hidden="true" />
      <div className="flex flex-col gap-px min-w-0 flex-1">
        <div
          data-rf-node-name
          className="text-xs font-semibold text-fg overflow-hidden text-ellipsis whitespace-nowrap"
        >
          {data.name}
        </div>
        <div className="text-[11px] text-fg-dim font-sans overflow-hidden text-ellipsis whitespace-nowrap">
          {data.subtitle}
        </div>
      </div>
      {data.findings && <FindingBadge findings={data.findings} />}
      <Handle
        type="source"
        position={Position.Right}
        isConnectable={false}
        className={HANDLE_CLASS}
      />
    </div>
  );
}

export const GraphNode = memo(GraphNodeImpl);
