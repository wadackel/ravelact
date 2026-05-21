import type { ReactNode } from "react";

export function Chip({ children }: { children: ReactNode }) {
  return (
    <span className="inline-block bg-bg-elev2 text-fg border border-border rounded-xl px-2.5 py-0.5 text-[11px] font-sans">
      {children}
    </span>
  );
}
