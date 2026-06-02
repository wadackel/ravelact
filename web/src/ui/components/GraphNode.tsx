import { memo } from "react";
import { Handle, Position } from "@xyflow/react";
import type { NodeKind } from "../../lib/types.ts";
import { SeverityDot, type SeverityTier } from "./ui/index.ts";

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

// Compact non-zero tally, severity order, e.g. `E1 H2 M1`. Kept as the
// accessible-name / tooltip text for the multi-dot badge.
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

// Severity tiers in display order, paired with the overlay-count field they
// read. Drives the multi-dot badge: one colored dot + count per present tier.
const BADGE_TIERS: ReadonlyArray<[SeverityTier, keyof FindingOverlay["counts"]]> = [
  ["error", "error"],
  ["high", "high"],
  ["medium", "medium"],
  ["low", "low"],
  ["info", "info"],
];

// Symbolic finding badge: a colored SeverityDot + count for each present tier
// (e.g. ●1 ●2 ●1), replacing the dense `E1 H2 M1` string. Colors are
// token-driven via SeverityDot. The compact tally remains the accessible name.
function FindingBadge({ findings }: { findings: FindingOverlay }) {
  const c = findings.counts;
  const label = compactCounts(c);
  return (
    <span
      data-testid="finding-badge"
      className="ml-1 shrink-0 inline-flex items-center gap-1"
      title={`${c.total} finding(s): ${label}`}
      aria-label={`${c.total} findings: ${label}`}
    >
      {BADGE_TIERS.filter(([, field]) => c[field] > 0).map(([severity, field]) => (
        <span
          key={severity}
          className="inline-flex items-center gap-1 rounded-full border border-border bg-bg-elev2 px-1.5 py-0.5"
        >
          <SeverityDot severity={severity} size="sm" />
          <span className="text-[10px] font-semibold font-mono text-fg-muted leading-none">
            {c[field]}
          </span>
        </span>
      ))}
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
