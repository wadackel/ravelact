// Per-rule source-tool badge for the cross-cutting findings list and the Panel
// findings rows. zizmor / actionlint get dedicated hues; every other source
// (ravelact, unknown external tools) falls back to the neutral `default`
// palette. Colors are token-driven (index.css `--color-source-*`).

// Map an arbitrary source label to one of the three color variants. The badge
// still displays the verbatim source string; only the color bucket collapses.
function sourceVariant(source: string): "zizmor" | "actionlint" | "default" {
  if (source === "zizmor" || source === "actionlint") return source;
  return "default";
}

const COLOR_BY_SOURCE =
  "data-[source=zizmor]:bg-source-zizmor-bg data-[source=zizmor]:text-source-zizmor-text data-[source=zizmor]:border-source-zizmor " +
  "data-[source=actionlint]:bg-source-actionlint-bg data-[source=actionlint]:text-source-actionlint-text data-[source=actionlint]:border-source-actionlint " +
  "data-[source=default]:bg-source-default-bg data-[source=default]:text-source-default-text data-[source=default]:border-source-default";

export type SourceBadgeProps = {
  source: string;
};

export function SourceBadge({ source }: SourceBadgeProps) {
  return (
    <span
      data-testid="source-badge"
      data-source={sourceVariant(source)}
      className={`inline-flex items-center rounded border px-1.5 py-px text-[10px] font-semibold tracking-wide font-sans ${COLOR_BY_SOURCE}`}
    >
      {source}
    </span>
  );
}
