import { type KeyboardEvent as ReactKeyboardEvent, useEffect, useMemo, useRef } from "react";
import type { TriggerSummary } from "../../lib/types.ts";
import { Kbd } from "../primitives/index.ts";

export type HeaderProps = {
  nodeCount: number;
  triggers: TriggerSummary[] | null;
  searchQuery: string;
  onSearchChange: (q: string) => void;
  onSearchEnter: () => void;
};

export function Header({
  nodeCount,
  triggers,
  searchQuery,
  onSearchChange,
  onSearchEnter,
}: HeaderProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);

  const counts = useMemo(() => {
    if (!triggers) return null;
    const entryWorkflows = triggers.reduce((a, x) => a + (x.entry_workflows ?? 0), 0);
    return { events: triggers.length, entryWorkflows };
  }, [triggers]);

  // Global ⌘K / Ctrl+K to focus the search input. Bound once at the
  // component scope, not on `window` directly — the cleanup unbinds
  // on unmount. If a future browser starts intercepting ⌘K (none do
  // for top-level pages today; Chrome reserves ⌘L), swap to `/`.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function onInputKeyDown(e: ReactKeyboardEvent<HTMLInputElement>) {
    if (e.key === "Escape") {
      e.preventDefault();
      onSearchChange("");
      inputRef.current?.blur();
    } else if (e.key === "Enter") {
      e.preventDefault();
      onSearchEnter();
    }
  }

  function handleFullscreen() {
    const root = document.documentElement;
    // Catch promise rejection (e.g. user gesture missing, Permissions
    // Policy blocks the call) so the unhandled-rejection warning does
    // not leak into the console.
    if (document.fullscreenElement) {
      document.exitFullscreen?.().catch(() => {});
    } else {
      root.requestFullscreen?.().catch(() => {});
    }
  }

  return (
    <header className="h-12 bg-bg-elev border-b border-border flex items-center px-4 gap-4">
      <h1 className="m-0 text-[13px] font-semibold tracking-[0.01em] text-fg">
        ravelact <span className="text-fg-muted font-normal">/ browse</span>
      </h1>
      <div
        id="stats"
        className="text-fg-muted font-sans text-xs whitespace-nowrap"
        aria-label="Graph statistics"
      >
        {counts && (
          <>
            <span className="text-fg font-semibold">{counts.events}</span> events
            <span className="inline-block mx-2 text-fg-dim">·</span>
            <span className="text-fg font-semibold">{counts.entryWorkflows}</span> entry workflows
            <span className="inline-block mx-2 text-fg-dim">·</span>
            <span className="text-fg font-semibold">{nodeCount}</span> nodes
          </>
        )}
      </div>
      <div className="flex-1" />
      <div className="flex items-center gap-2 bg-bg border border-border rounded-md py-1 pr-2 pl-2.5 w-[280px] text-fg-dim transition focus-within:border-accent focus-within:shadow-[0_0_0_2px_color-mix(in_srgb,var(--color-accent)_18%,transparent)]">
        <span className="inline-flex text-fg-dim" aria-hidden="true">
          <svg
            width="14"
            height="14"
            viewBox="0 0 16 16"
            fill="none"
            xmlns="http://www.w3.org/2000/svg"
          >
            <circle cx="7" cy="7" r="4.5" stroke="currentColor" strokeWidth="1.5" />
            <path
              d="M10.5 10.5L13 13"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
        </span>
        <input
          ref={inputRef}
          type="search"
          value={searchQuery}
          onChange={(e) => onSearchChange(e.target.value)}
          onKeyDown={onInputKeyDown}
          placeholder="Search nodes, files, triggers..."
          aria-label="Search nodes, files, and triggers"
          className="flex-1 border-0 outline-none bg-transparent text-[12.5px] text-fg cursor-text min-w-0 placeholder:text-fg-dim"
        />
        <Kbd>⌘K</Kbd>
      </div>
      <button
        className="w-7 h-7 inline-flex items-center justify-center bg-transparent border border-transparent rounded-md text-fg-muted cursor-pointer p-0 hover:text-fg hover:bg-bg-elev2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
        type="button"
        onClick={handleFullscreen}
        aria-label="Toggle fullscreen"
      >
        <svg
          width="14"
          height="14"
          viewBox="0 0 16 16"
          fill="none"
          xmlns="http://www.w3.org/2000/svg"
          aria-hidden="true"
        >
          <path
            d="M2 5.5V2.5C2 2.22 2.22 2 2.5 2H5.5"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
          <path
            d="M14 5.5V2.5C14 2.22 13.78 2 13.5 2H10.5"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
          <path
            d="M2 10.5V13.5C2 13.78 2.22 14 2.5 14H5.5"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
          <path
            d="M14 10.5V13.5C14 13.78 13.78 14 13.5 14H10.5"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          />
        </svg>
      </button>
    </header>
  );
}
