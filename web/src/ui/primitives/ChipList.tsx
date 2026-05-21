import type { ReactNode } from "react";

export function ChipList({ children }: { children: ReactNode }) {
  return <div className="flex flex-wrap gap-1">{children}</div>;
}
