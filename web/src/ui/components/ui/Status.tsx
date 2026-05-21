import type { ReactNode } from "react";

export type StatusProps = {
  type: "loading" | "empty" | "error";
  children?: ReactNode;
};

export function Status({ type, children }: StatusProps) {
  if (type === "loading") {
    return (
      <div role="status" className="text-fg-muted italic py-3">
        {children ?? "Loading…"}
      </div>
    );
  }
  if (type === "error") {
    return (
      <div
        role="alert"
        className="text-warn bg-warn-bg border border-warn rounded-md p-3 text-xs font-sans"
      >
        {children}
      </div>
    );
  }
  return <div className="text-fg-dim text-center py-6 italic">{children}</div>;
}
