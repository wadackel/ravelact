// Severity tiers, most → least severe. Shared with the GraphNode finding
// badge, the FindingsFloat cross-cutting list, and the Panel findings rows so
// the five colors live in exactly one place (index.css `--color-sev-*`).
export const SEVERITY_TIERS = ["error", "high", "medium", "low", "info"] as const;
export type SeverityTier = (typeof SEVERITY_TIERS)[number];

// Runtime guard narrowing an arbitrary (proto-sourced, open-shape) string to a
// SeverityTier — mirrors the `isNodeKind` guard so severity strings from the
// wire are validated rather than blindly `as`-cast.
export function isSeverityTier(s: string): s is SeverityTier {
  return (SEVERITY_TIERS as ReadonlyArray<string>).includes(s);
}

// data-attribute arbitrary variants (Kind.tsx pattern) so the dot color is
// token-driven — no inline hex. `--color-sev-<tier>` → `bg-sev-<tier>`.
const COLOR_BY_SEVERITY =
  "data-[severity=error]:bg-sev-error " +
  "data-[severity=high]:bg-sev-high " +
  "data-[severity=medium]:bg-sev-medium " +
  "data-[severity=low]:bg-sev-low " +
  "data-[severity=info]:bg-sev-info";

const SIZE = {
  sm: "w-[6px] h-[6px]",
  md: "w-2 h-2",
} as const;

export type SeverityDotProps = {
  severity: SeverityTier;
  size?: keyof typeof SIZE;
  // Native tooltip; the dot itself is decorative (aria-hidden) and relies on
  // the surrounding label/count for the accessible name.
  title?: string;
};

export function SeverityDot({ severity, size = "md", title }: SeverityDotProps) {
  return (
    <span
      data-testid="severity-dot"
      data-severity={severity}
      title={title}
      aria-hidden="true"
      className={`inline-block shrink-0 rounded-full ${SIZE[size]} ${COLOR_BY_SEVERITY}`}
    />
  );
}
