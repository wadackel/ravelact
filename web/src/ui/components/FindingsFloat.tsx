import {
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type { FindingContext, FindingFacets } from "../../lib/graph-filter.ts";
import type { FindingWithNode, NodeKind, TriggerSummary } from "../../lib/types.ts";
import { isSeverityTier, SeverityDot, type SeverityTier, Status } from "./ui/index.ts";
import { SourceBadge } from "./ui/SourceBadge.tsx";

// Severity tiers, most → least severe (matches SeverityDot ordering + the
// node-card / Panel surfaces).
const SEVERITIES: readonly SeverityTier[] = ["error", "high", "medium", "low", "info"];

// Context facets and their lens labels. `write` is workflow-only — action
// nodes never carry permission context.
const CONTEXTS: ReadonlyArray<{ key: FindingContext; label: string }> = [
  { key: "reachable", label: "Reachable from risky" },
  { key: "orphan", label: "Orphan" },
  { key: "write", label: "Write perms" },
];

const FLOAT_TABS = ["findings", "events"] as const;
type FloatTab = (typeof FLOAT_TABS)[number];

// Toggle `value` within an active Set. An empty result collapses back to
// `null` (facet inactive) so `findingsActive` reports no constraint. Mirrors
// the helper the old FindingsFilter used.
function toggle<T extends string>(set: ReadonlySet<T> | null, value: T): ReadonlySet<T> | null {
  const next = new Set<T>(set ?? []);
  if (next.has(value)) {
    next.delete(value);
  } else {
    next.add(value);
  }
  return next.size === 0 ? null : next;
}

function isNodeKind(k: string): k is NodeKind {
  return (
    k === "workflow" ||
    k === "local-action" ||
    k === "external-action" ||
    k === "external-workflow" ||
    k === "docker"
  );
}

export type FindingsFloatProps = {
  // Findings tab. `findings` is the cross-cutting list (ListFindings); empty
  // when browse ran without `--findings` or every finding was unresolved.
  hasFindings: boolean;
  findings: readonly FindingWithNode[];
  availableSources: readonly string[];
  facets: FindingFacets;
  onChangeFacets: (next: FindingFacets) => void;
  // Select + fit the node a finding anchors to (App wires graph fit).
  onSelectFinding: (nodeId: string, kind: NodeKind) => void;

  // Events tab (moved from OverviewPane).
  triggers: TriggerSummary[] | null;
  selectedEvent: string | null;
  onSelectEvent: (event: string | null) => void;
};

/**
 * Cross-cutting floating panel: a Findings lens (severity dots + context lens
 * + source filter + the estate-wide findings list) and an Events lens (the
 * trigger summary). Replaces the old top-left FindingsFilter float and the
 * right-pane OverviewPane, so the right pane is now node-only.
 */
export function FindingsFloat({
  hasFindings,
  findings,
  availableSources,
  facets,
  onChangeFacets,
  onSelectFinding,
  triggers,
  selectedEvent,
  onSelectEvent,
}: FindingsFloatProps) {
  const rootRef = useRef<HTMLElement | null>(null);
  const tabs: readonly FloatTab[] = hasFindings ? FLOAT_TABS : ["events"];
  const [tab, setTab] = useState<FloatTab>(hasFindings ? "findings" : "events");
  const [collapsed, setCollapsed] = useState(false);
  const activeTab: FloatTab = tabs.includes(tab) ? tab : "events";

  // Roving-tabindex arrow-key navigation for the tablist, mirroring the node
  // Panel's tab keyboard handling (ARIA APG Tabs pattern).
  function onTabKeyDown(e: ReactKeyboardEvent<HTMLElement>) {
    const idx = tabs.indexOf(activeTab);
    let next: FloatTab | null = null;
    if (e.key === "ArrowRight") next = tabs[(idx + 1) % tabs.length] ?? activeTab;
    else if (e.key === "ArrowLeft") next = tabs[(idx - 1 + tabs.length) % tabs.length] ?? activeTab;
    else if (e.key === "Home") next = tabs[0] ?? activeTab;
    else if (e.key === "End") next = tabs[tabs.length - 1] ?? activeTab;
    if (next) {
      e.preventDefault();
      setTab(next);
      document.getElementById(`float-tab-${next}`)?.focus();
    }
  }

  // Escape clears the event selection when focus is within the float. Scoped
  // to the panel (not window) so it never competes with the node Panel's own
  // window-level Escape-to-close handler. Moved from OverviewPane.
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape" && selectedEvent !== null) {
        e.preventDefault();
        onSelectEvent(null);
      }
    }
    root.addEventListener("keydown", onKey);
    return () => root.removeEventListener("keydown", onKey);
  }, [selectedEvent, onSelectEvent]);

  // Per-tier source-severity counts across the estate (mirrors the node
  // card's source-severity tally so the dots agree across surfaces).
  const severityCounts = useMemo(() => {
    const counts: Record<SeverityTier, number> = { error: 0, high: 0, medium: 0, low: 0, info: 0 };
    for (const row of findings) {
      const sev = row.finding?.sourceSeverity;
      if (sev && isSeverityTier(sev)) counts[sev] += 1;
    }
    return counts;
  }, [findings]);

  return (
    <aside
      ref={rootRef}
      aria-label="Findings and events"
      data-testid="findings-float"
      // Height is bounded by the parent band (top→bottom inset in App); the
      // body scrolls within. `pointer-events-auto` re-enables interaction that
      // the parent's `pointer-events-none` (graph click-through) suppresses.
      className="flex flex-col max-h-full pointer-events-auto rounded-md border border-border bg-bg shadow-[0_4px_12px_rgba(0,0,0,0.08)] overflow-hidden"
    >
      <div className="flex items-center justify-between border-b border-border px-2 py-1.5">
        <div
          role="tablist"
          aria-label="Cross-cutting views"
          className="flex gap-1"
          onKeyDown={onTabKeyDown}
        >
          {tabs.map((t) => {
            const count = t === "findings" ? findings.length : (triggers?.length ?? 0);
            return (
              <button
                key={t}
                id={`float-tab-${t}`}
                role="tab"
                type="button"
                aria-selected={activeTab === t}
                aria-controls="float-tabpanel"
                tabIndex={activeTab === t ? 0 : -1}
                onClick={() => setTab(t)}
                className="inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] font-medium text-fg-muted hover:text-fg hover:bg-bg-elev aria-selected:text-fg aria-selected:bg-bg-elev2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
              >
                <TabIcon tab={t} />
                <span className="capitalize">{t}</span>
                <span className="inline-flex items-center justify-center min-w-[18px] h-[16px] px-1 rounded-full text-[10px] bg-bg-elev2 text-fg-muted">
                  {count}
                </span>
              </button>
            );
          })}
        </div>
        <button
          type="button"
          aria-expanded={!collapsed}
          aria-label={collapsed ? "Expand panel" : "Collapse panel"}
          onClick={() => setCollapsed((c) => !c)}
          className="w-6 h-6 inline-flex items-center justify-center rounded-sm text-fg-muted hover:text-fg hover:bg-bg-elev2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
        >
          <Chevron collapsed={collapsed} />
        </button>
      </div>

      {!collapsed && (
        <div
          id="float-tabpanel"
          role="tabpanel"
          aria-labelledby={`float-tab-${activeTab}`}
          tabIndex={0}
          className="flex-1 min-h-0 overflow-y-auto"
        >
          {activeTab === "findings" && (
            <FindingsTab
              findings={findings}
              severityCounts={severityCounts}
              availableSources={availableSources}
              facets={facets}
              onChangeFacets={onChangeFacets}
              onSelectFinding={onSelectFinding}
            />
          )}
          {activeTab === "events" && (
            <EventsTab
              triggers={triggers}
              selectedEvent={selectedEvent}
              onSelectEvent={onSelectEvent}
            />
          )}
        </div>
      )}
    </aside>
  );
}

// Per-tab leading glyph: a shield-alert for Findings, a lightning bolt for
// Events. Decorative (the tab text is the accessible name).
function TabIcon({ tab }: { tab: FloatTab }) {
  const common = {
    width: 14,
    height: 14,
    viewBox: "0 0 16 16",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.5,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    "aria-hidden": true,
    xmlns: "http://www.w3.org/2000/svg",
  };
  if (tab === "findings") {
    return (
      <svg {...common}>
        <path d="M8 1.5l5 1.8v4.2c0 3-2.1 5.2-5 6.5-2.9-1.3-5-3.5-5-6.5V3.3l5-1.8z" />
        <path d="M8 5.5v3" />
        <path d="M8 10.5h.01" />
      </svg>
    );
  }
  return (
    <svg {...common}>
      <path d="M8.5 1.5L3 9h4l-.5 5.5L12 7H8l.5-5.5z" />
    </svg>
  );
}

function Chevron({ collapsed }: { collapsed: boolean }) {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
      className={collapsed ? "-rotate-90 transition-transform" : "transition-transform"}
    >
      <path
        d="M4 6L8 10L12 6"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// Severity capitalisation for the section labels.
const SEVERITY_LABEL: Record<SeverityTier, string> = {
  error: "Error",
  high: "High",
  medium: "Medium",
  low: "Low",
  info: "Info",
};

function SectionLabel({ children }: { children: ReactNode }) {
  return (
    <div className="text-fg-dim text-[10.5px] uppercase tracking-wider font-semibold mb-1.5">
      {children}
    </div>
  );
}

function FindingsTab({
  findings,
  severityCounts,
  availableSources,
  facets,
  onChangeFacets,
  onSelectFinding,
}: {
  findings: readonly FindingWithNode[];
  severityCounts: Record<SeverityTier, number>;
  availableSources: readonly string[];
  facets: FindingFacets;
  onChangeFacets: (next: FindingFacets) => void;
  onSelectFinding: (nodeId: string, kind: NodeKind) => void;
}) {
  const presentSeverities = SEVERITIES.filter((s) => severityCounts[s] > 0);
  return (
    <div className="p-3 flex flex-col gap-3">
      {presentSeverities.length > 0 && (
        <div>
          <SectionLabel>Severity</SectionLabel>
          <div className="flex flex-wrap gap-1.5">
            {presentSeverities.map((s) => (
              <button
                key={s}
                type="button"
                aria-pressed={facets.severities?.has(s) ?? false}
                onClick={() =>
                  onChangeFacets({ ...facets, severities: toggle(facets.severities, s) })
                }
                className="inline-flex items-center gap-1.5 rounded-xl border border-border bg-bg-elev2 px-2 py-1 text-[11px] font-sans text-fg cursor-pointer transition hover:bg-bg-elev aria-pressed:bg-[color-mix(in_srgb,var(--color-accent)_18%,transparent)] aria-pressed:border-accent aria-pressed:text-accent aria-pressed:font-medium focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
              >
                <SeverityDot severity={s} size="sm" />
                <span>{SEVERITY_LABEL[s]}</span>
                <span className="text-fg-muted">{severityCounts[s]}</span>
              </button>
            ))}
          </div>
        </div>
      )}

      <div>
        <SectionLabel>Context</SectionLabel>
        <div className="flex flex-wrap gap-1.5">
          {CONTEXTS.map(({ key, label }) => (
            <button
              key={key}
              type="button"
              aria-pressed={facets.contexts?.has(key) ?? false}
              onClick={() => onChangeFacets({ ...facets, contexts: toggle(facets.contexts, key) })}
              className="inline-block rounded-xl border border-border bg-bg-elev2 px-2.5 py-1 text-[11px] font-sans text-fg cursor-pointer transition hover:bg-bg-elev aria-pressed:bg-[color-mix(in_srgb,var(--color-accent)_18%,transparent)] aria-pressed:border-accent aria-pressed:text-accent aria-pressed:font-medium focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {availableSources.length > 1 && (
        <div>
          <SectionLabel>Source</SectionLabel>
          <div className="flex flex-wrap gap-1.5">
            {availableSources.map((src) => (
              <button
                key={src}
                type="button"
                aria-pressed={facets.sources?.has(src) ?? false}
                onClick={() => onChangeFacets({ ...facets, sources: toggle(facets.sources, src) })}
                className="inline-block rounded-xl border border-border bg-bg-elev2 px-2.5 py-1 text-[11px] font-sans text-fg cursor-pointer transition hover:bg-bg-elev aria-pressed:bg-[color-mix(in_srgb,var(--color-accent)_18%,transparent)] aria-pressed:border-accent aria-pressed:text-accent aria-pressed:font-medium focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
              >
                {src}
              </button>
            ))}
          </div>
        </div>
      )}

      <div className="border-t border-border-soft -mx-3" />

      {findings.length === 0 ? (
        <Status type="empty">No node-attached findings</Status>
      ) : (
        <ul className="list-none p-0 m-0 flex flex-col gap-0.5" aria-label="Findings">
          {findings.map((row, i) => (
            <FindingListRow
              key={`${row.nodeId}:${row.finding?.ruleId ?? ""}:${row.finding?.file ?? ""}:${row.finding?.line ?? 0}:${i}`}
              row={row}
              onSelectFinding={onSelectFinding}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function FindingListRow({
  row,
  onSelectFinding,
}: {
  row: FindingWithNode;
  onSelectFinding: (nodeId: string, kind: NodeKind) => void;
}) {
  const f = row.finding;
  if (!f) return null;
  const severity: SeverityTier = isSeverityTier(f.sourceSeverity) ? f.sourceSeverity : "info";
  const kind = row.nodeKind;
  return (
    <li>
      <button
        type="button"
        data-testid="float-finding-row"
        onClick={() => {
          if (isNodeKind(kind)) onSelectFinding(row.nodeId, kind);
        }}
        className="w-full text-left rounded-md border border-transparent px-2 py-1.5 cursor-pointer transition hover:bg-bg-elev hover:border-border focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
      >
        <div className="flex items-center gap-2">
          <SeverityDot severity={severity} size="sm" title={severity} />
          <span className="font-mono text-[11px] text-fg break-all flex-1 min-w-0">{f.ruleId}</span>
          <SourceBadge source={f.source} />
        </div>
        {f.message && (
          <div className="text-fg-muted text-[11px] mt-0.5 break-words">{f.message}</div>
        )}
        <div className="text-fg-dim text-[10.5px] mt-0.5 font-mono break-all">{row.nodeId}</div>
        <div className="text-fg-dim text-[10.5px] font-mono break-all">
          {f.file}
          {f.line > 0 ? `:${f.line}` : ""}
        </div>
      </button>
    </li>
  );
}

function EventsTab({
  triggers,
  selectedEvent,
  onSelectEvent,
}: {
  triggers: TriggerSummary[] | null;
  selectedEvent: string | null;
  onSelectEvent: (event: string | null) => void;
}) {
  return (
    <div className="p-3">
      <div className="flex items-center justify-between mb-2">
        <SectionLabel>Events</SectionLabel>
        {selectedEvent !== null && (
          <button
            type="button"
            className="bg-transparent border border-border text-fg-muted text-[11px] px-2 py-0.5 rounded-sm cursor-pointer normal-case tracking-normal font-normal hover:text-fg hover:bg-bg-elev2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
            onClick={() => onSelectEvent(null)}
            aria-label="Clear event selection"
          >
            Clear
          </button>
        )}
      </div>
      {triggers === null && <Status type="loading" />}
      {triggers !== null && triggers.length === 0 && (
        <Status type="empty">No events declared</Status>
      )}
      {triggers !== null && triggers.length > 0 && (
        <ul className="list-none p-0 m-0 flex flex-col gap-0.5" aria-label="Events">
          {triggers.map((t) => {
            const isSelected = selectedEvent === t.event;
            return (
              <li key={t.event}>
                <button
                  type="button"
                  className="w-full flex items-center justify-between bg-transparent border border-transparent rounded-md py-1.5 px-2.5 cursor-pointer font-sans text-[12.5px] text-fg text-left transition hover:bg-bg-elev hover:border-border aria-pressed:bg-[color-mix(in_srgb,var(--color-accent)_8%,var(--color-bg))] aria-pressed:border-accent aria-pressed:text-accent aria-pressed:font-medium focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
                  aria-pressed={isSelected}
                  onClick={() => onSelectEvent(isSelected ? null : t.event)}
                >
                  <span className="font-sans text-xs">{t.event}</span>
                  <span
                    data-selected={isSelected ? "true" : undefined}
                    className="inline-flex items-center justify-center min-w-[22px] h-[18px] px-1.5 rounded-full text-[11px] font-sans bg-bg-elev2 text-fg-muted data-[selected=true]:bg-[color-mix(in_srgb,var(--color-accent)_18%,transparent)] data-[selected=true]:text-accent"
                    aria-label={`${t.entryWorkflows} entry workflow${t.entryWorkflows === 1 ? "" : "s"}`}
                  >
                    {t.entryWorkflows}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
