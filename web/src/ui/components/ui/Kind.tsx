import { kindBadge, kindLabel } from "../../../lib/kind-format.ts";
import type { NodeKind } from "../../../lib/types.ts";

export type KindProps = {
  kind: NodeKind;
  variant: "badge" | "pill";
  "aria-hidden"?: boolean | "true" | "false";
};

const BASE_BADGE =
  "inline-flex items-center justify-center min-w-[24px] h-5 px-1.5 text-[10px] font-bold tracking-wider rounded-sm border flex-shrink-0 font-sans";
const BASE_PILL = "inline-block px-2.5 py-0.5 text-[11.5px] font-medium rounded-full border";

// Per-kind background / text / border via data-attribute arbitrary
// variants. Combined as one long class string so we don't need
// clsx-style conditional joining.
const COLOR_BY_KIND =
  "data-[kind=workflow]:bg-kind-workflow-fill data-[kind=workflow]:text-kind-workflow-text data-[kind=workflow]:border-kind-workflow-border " +
  "data-[kind=local-action]:bg-kind-local-action-fill data-[kind=local-action]:text-kind-local-action-text data-[kind=local-action]:border-kind-local-action-border " +
  "data-[kind=external-action]:bg-kind-external-action-fill data-[kind=external-action]:text-kind-external-action-text data-[kind=external-action]:border-kind-external-action-border " +
  "data-[kind=external-workflow]:bg-kind-external-workflow-fill data-[kind=external-workflow]:text-kind-external-workflow-text data-[kind=external-workflow]:border-kind-external-workflow-border " +
  "data-[kind=docker]:bg-kind-docker-fill data-[kind=docker]:text-kind-docker-text data-[kind=docker]:border-kind-docker-border";

export function Kind({ kind, variant, "aria-hidden": ariaHidden }: KindProps) {
  const base = variant === "badge" ? BASE_BADGE : BASE_PILL;
  const text = variant === "badge" ? kindBadge(kind) : kindLabel(kind);
  return (
    <span
      className={`${base} ${COLOR_BY_KIND}`}
      data-kind={kind}
      data-variant={variant}
      aria-hidden={ariaHidden}
    >
      {text}
    </span>
  );
}
