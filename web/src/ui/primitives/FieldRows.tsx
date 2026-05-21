import type { ReactNode } from "react";

// Container that hosts a vertical list of `<FieldValue>` rows with the
// per-row divider styling. Uses Tailwind arbitrary descendant selectors
// to express the `:last-child` exception inline, so no extra CSS rule
// is needed.
export function FieldRows({ children }: { children: ReactNode }) {
  return (
    <div
      data-field-rows
      className="[&>[data-field-value][data-mono=true]]:py-1 [&>[data-field-value][data-mono=true]:not(:last-child)]:border-b [&>[data-field-value][data-mono=true]:not(:last-child)]:border-border-soft"
    >
      {children}
    </div>
  );
}
