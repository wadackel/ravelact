import type { ReactNode } from "react";
import type { FindingContext, FindingFacets } from "../../lib/graph-filter.ts";

// Severity tiers, most → least severe (matches the badge ordering).
const SEVERITIES = ["error", "high", "medium", "low", "info"] as const;

// Context facets and their user-facing labels. `write` is workflow-only —
// action nodes never carry permission context, so the label discloses that.
const CONTEXTS: ReadonlyArray<{ key: FindingContext; label: string }> = [
  { key: "reachable", label: "reachable from risky" },
  { key: "orphan", label: "orphan" },
  { key: "write", label: "write perms (workflows)" },
];

export type FindingsFilterProps = {
  facets: FindingFacets;
  onChange: (next: FindingFacets) => void;
  // Distinct sources present in the graph (drives the source facet options).
  availableSources: readonly string[];
};

// Toggle `value` within an active Set. An empty result collapses back to
// `null` (facet inactive) so `findingsActive` reports no constraint.
function toggle<T extends string>(set: ReadonlySet<T> | null, value: T): ReadonlySet<T> | null {
  const next = new Set<T>(set ?? []);
  if (next.has(value)) {
    next.delete(value);
  } else {
    next.add(value);
  }
  return next.size === 0 ? null : next;
}

function FacetChip({
  label,
  active,
  onToggle,
}: {
  label: string;
  active: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onToggle}
      className="inline-block bg-bg-elev2 text-fg border border-border rounded-xl px-2.5 py-1 text-[11px] font-sans cursor-pointer transition hover:bg-bg-elev aria-pressed:bg-[color-mix(in_srgb,var(--color-accent)_18%,transparent)] aria-pressed:border-accent aria-pressed:text-accent aria-pressed:font-medium focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
    >
      {label}
    </button>
  );
}

function FacetGroup({ legend, children }: { legend: string; children: ReactNode }) {
  return (
    <fieldset className="border-0 p-0 m-0">
      <legend className="text-[11px] text-fg-muted font-medium mb-1 p-0">{legend}</legend>
      <div className="flex flex-wrap gap-1.5">{children}</div>
    </fieldset>
  );
}

/**
 * Findings facet controls. Each facet narrows the graph to finding-bearing
 * nodes matching the selection (AND across facets; OR within). Source has a
 * single option today (zizmor) and is hidden when only one source exists.
 */
export function FindingsFilter({ facets, onChange, availableSources }: FindingsFilterProps) {
  return (
    <div
      data-testid="findings-filter"
      className="flex flex-col gap-3 p-3 border border-border rounded-md bg-bg"
      aria-label="Findings filters"
    >
      <FacetGroup legend="Severity">
        {SEVERITIES.map((s) => (
          <FacetChip
            key={s}
            label={s}
            active={facets.severities?.has(s) ?? false}
            onToggle={() => onChange({ ...facets, severities: toggle(facets.severities, s) })}
          />
        ))}
      </FacetGroup>

      {availableSources.length > 1 && (
        <FacetGroup legend="Source">
          {availableSources.map((src) => (
            <FacetChip
              key={src}
              label={src}
              active={facets.sources?.has(src) ?? false}
              onToggle={() => onChange({ ...facets, sources: toggle(facets.sources, src) })}
            />
          ))}
        </FacetGroup>
      )}

      <FacetGroup legend="Context">
        {CONTEXTS.map(({ key, label }) => (
          <FacetChip
            key={key}
            label={label}
            active={facets.contexts?.has(key) ?? false}
            onToggle={() => onChange({ ...facets, contexts: toggle(facets.contexts, key) })}
          />
        ))}
      </FacetGroup>
    </div>
  );
}
