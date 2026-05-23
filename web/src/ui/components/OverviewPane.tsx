import { useEffect, useRef } from "react";
import type { TriggerSummary } from "../../lib/types.ts";
import { ResizableRightPane } from "./ResizableRightPane.tsx";
import { Status } from "./ui/index.ts";

export type OverviewPaneProps = {
  triggers: TriggerSummary[] | null;
  selectedEvent: string | null;
  onSelectEvent: (event: string | null) => void;
  width: number;
  onWidthChange: (next: number) => void;
};

// Right-pane overview shown when no node is selected. Surfaces the
// `ravelact triggers` summary in clickable form: clicking an event
// drives `App` to fetch /api/event-impact, which Graph then uses to
// fade everything that is not transitively reached from a workflow
// triggered by that event — mirroring the CLI `ravelact trace
// --event <name>` reachable set in graph form.
export function OverviewPane({
  triggers,
  selectedEvent,
  onSelectEvent,
  width,
  onWidthChange,
}: OverviewPaneProps) {
  const rootRef = useRef<HTMLElement | null>(null);

  // Escape clears the selection when focus is within the overview
  // pane. Scoped to the pane (not window) so it does not interfere
  // with the panel close handler when a node is selected (in which
  // case OverviewPane is not mounted).
  useEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape" && selectedEvent !== null) {
        e.preventDefault();
        onSelectEvent(null);
      }
    }
    root.addEventListener("keydown", onKey);
    return () => root.removeEventListener("keydown", onKey);
  }, [selectedEvent, onSelectEvent]);

  return (
    <ResizableRightPane width={width} onWidthChange={onWidthChange}>
      <aside
        className="h-full border-l border-border bg-bg flex flex-col overflow-y-auto p-4"
        ref={rootRef}
        aria-label="Graph overview"
      >
        <div>
          <div className="flex items-center justify-between text-fg-dim text-[10.5px] uppercase tracking-wider font-semibold mb-2">
            <span>Events</span>
            {selectedEvent !== null && (
              <button
                type="button"
                className="bg-transparent border border-border text-fg-muted text-[11px] px-2 py-0.5 rounded-sm cursor-pointer normal-case tracking-normal font-normal hover:text-fg hover:bg-bg-elev2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
                onClick={() => onSelectEvent(null)}
                aria-label="Clear event selection"
              >
                Clear
              </button>
            )}
          </div>
          {triggers === null && <Status type="loading" />}
          {triggers !== null && triggers.length === 0 && (
            <Status type="empty">No events declared</Status>
          )}
          {triggers !== null && triggers.length > 0 && (
            <ul className="list-none p-0 m-0 flex flex-col gap-0.5" aria-label="Events">
              {triggers.map((t) => {
                const isSelected = selectedEvent === t.event;
                return (
                  <li key={t.event}>
                    <button
                      type="button"
                      className="w-full flex items-center justify-between bg-transparent border border-transparent rounded-md py-1.5 px-2.5 cursor-pointer font-sans text-[12.5px] text-fg text-left transition hover:bg-bg-elev hover:border-border aria-pressed:bg-[color-mix(in_srgb,var(--color-accent)_8%,var(--color-bg))] aria-pressed:border-accent aria-pressed:text-accent aria-pressed:font-medium focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
                      aria-pressed={isSelected}
                      onClick={() => onSelectEvent(isSelected ? null : t.event)}
                    >
                      <span className="font-sans text-xs">{t.event}</span>
                      <span
                        data-selected={isSelected ? "true" : undefined}
                        className="inline-flex items-center justify-center min-w-[22px] h-[18px] px-1.5 rounded-full text-[11px] font-sans bg-bg-elev2 text-fg-muted data-[selected=true]:bg-[color-mix(in_srgb,var(--color-accent)_18%,transparent)] data-[selected=true]:text-accent"
                        aria-label={`${t.entryWorkflows} entry workflow${
                          t.entryWorkflows === 1 ? "" : "s"
                        }`}
                      >
                        {t.entryWorkflows}
                      </span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </aside>
    </ResizableRightPane>
  );
}
