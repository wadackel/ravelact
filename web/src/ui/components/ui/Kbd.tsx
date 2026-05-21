import type { ReactNode } from "react";

export function Kbd({ children }: { children: ReactNode }) {
  return (
    <kbd
      className="inline-flex items-center gap-px text-[11px] text-fg-muted bg-bg-elev2 border border-border rounded-sm px-1.5 py-px font-sans leading-snug"
      aria-hidden="true"
    >
      {children}
    </kbd>
  );
}
