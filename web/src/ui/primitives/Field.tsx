import type { ReactNode } from "react";

export type FieldProps = {
  label: ReactNode;
  // When true, sets `data-mono="true"` on the value slot so the
  // FieldRows divider rule applies and font-size shrinks. Despite the
  // historical name, the font remains sans — only `.tree` (trace tab
  // pre) renders true monospace.
  mono?: boolean;
  children: ReactNode;
};

export function Field({ label, mono, children }: FieldProps) {
  return (
    <div className="mb-4">
      <div className="text-fg-dim text-[10.5px] uppercase tracking-wider mb-1 font-semibold">
        {label}
      </div>
      <FieldValue mono={mono}>{children}</FieldValue>
    </div>
  );
}

export type FieldValueProps = {
  mono?: boolean;
  children: ReactNode;
};

// Stand-alone value slot — used by Field internally, and exposed for
// label-less list rows that live inside a FieldRows container
// (Impact tab). The `data-field-value` attribute is the canonical
// hook FieldRows uses to apply its row divider rule.
export function FieldValue({ mono, children }: FieldValueProps) {
  const base = "text-fg break-all";
  const monoClasses = mono ? " text-xs" : "";
  return (
    <div data-field-value data-mono={mono ? "true" : undefined} className={base + monoClasses}>
      {children}
    </div>
  );
}
