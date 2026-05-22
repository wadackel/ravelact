import { type KeyboardEvent as ReactKeyboardEvent, useEffect, useReducer } from "react";
import { fetchImpact, fetchNode, fetchTrace } from "../../lib/api.ts";
import { githubUrlFor } from "../../lib/github-url.ts";
import { renderTraceTree } from "../../lib/trace-render.ts";
import type {
  ImpactResponse,
  NodeKind,
  NodeResponse,
  RepoInfo,
  TraceResponse,
} from "../../lib/types.ts";
import { Chip, ChipList, Field, FieldRows, FieldValue, Kind, Status } from "./ui/index.ts";

export type Tab = "details" | "triggers" | "impact" | "trace";
const TABS: ReadonlyArray<Tab> = ["details", "triggers", "impact", "trace"];

type State = {
  // `undefined` means "not fetched yet"; `null` means "fetched but 404".
  details: NodeResponse | null | undefined;
  detailsError: string | null;
  impact: ImpactResponse | null | undefined;
  impactError: string | null;
  trace: TraceResponse | null | undefined;
  traceError: string | null;
};

type Action =
  | { type: "reset" }
  | { type: "details"; data: NodeResponse | null }
  | { type: "details-error"; message: string }
  | { type: "impact"; data: ImpactResponse | null }
  | { type: "impact-error"; message: string }
  | { type: "trace"; data: TraceResponse | null }
  | { type: "trace-error"; message: string };

const initialState: State = {
  details: undefined,
  detailsError: null,
  impact: undefined,
  impactError: null,
  trace: undefined,
  traceError: null,
};

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "reset":
      return initialState;
    case "details":
      return { ...state, details: action.data, detailsError: null };
    case "details-error":
      return { ...state, detailsError: action.message };
    case "impact":
      return { ...state, impact: action.data, impactError: null };
    case "impact-error":
      return { ...state, impactError: action.message };
    case "trace":
      return { ...state, trace: action.data, traceError: null };
    case "trace-error":
      return { ...state, traceError: action.message };
  }
}

export type PanelProps = {
  openFor: { id: string; kind: NodeKind } | null;
  onClose: () => void;
  repoInfo: RepoInfo | null;
  tab: Tab;
  onTabChange: (tab: Tab) => void;
};

export function Panel({ openFor, onClose, repoInfo, tab, onTabChange }: PanelProps) {
  const [state, dispatch] = useReducer(reducer, initialState);

  // Fetch on demand. The effect reads `state.{details,impact,trace}`
  // from the deps array so the early-return correctly skips re-fetching
  // an already-populated slice. Ownership split: `App.tsx` owns the
  // active tab (so it survives node-to-node switches) and passes it in
  // as `tab` + `onTabChange`. Panel owns the per-node data slices, and
  // `App.tsx` still passes `key={openFor.id}` on <Panel> so a node
  // change remounts a fresh instance with initial state — invalidating
  // the cached per-node data without touching the active tab.
  useEffect(() => {
    if (!openFor) return;
    const innerId = openFor.id.replace(/^(?:wf|la|ea|ew|dk):/, "");
    let cancelled = false;
    if ((tab === "details" || tab === "triggers") && state.details === undefined) {
      fetchNode(openFor.kind, innerId)
        .then((data) => {
          if (!cancelled) dispatch({ type: "details", data });
        })
        .catch((e: unknown) => {
          if (!cancelled) {
            dispatch({
              type: "details-error",
              message: e instanceof Error ? e.message : String(e),
            });
          }
        });
    } else if (tab === "impact" && state.impact === undefined) {
      fetchImpact(innerId)
        .then((data) => {
          if (!cancelled) dispatch({ type: "impact", data });
        })
        .catch((e: unknown) => {
          if (!cancelled) {
            dispatch({
              type: "impact-error",
              message: e instanceof Error ? e.message : String(e),
            });
          }
        });
    } else if (tab === "trace" && state.trace === undefined) {
      fetchTrace(innerId)
        .then((data) => {
          if (!cancelled) dispatch({ type: "trace", data });
        })
        .catch((e: unknown) => {
          if (!cancelled) {
            dispatch({
              type: "trace-error",
              message: e instanceof Error ? e.message : String(e),
            });
          }
        });
    }
    return () => {
      cancelled = true;
    };
  }, [openFor, tab, state.details, state.impact, state.trace]);

  // Escape closes the panel.
  useEffect(() => {
    if (!openFor) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [openFor, onClose]);

  function onTabKeyDown(e: ReactKeyboardEvent<HTMLElement>) {
    const idx = TABS.indexOf(tab);
    let next: Tab | null = null;
    if (e.key === "ArrowRight") {
      next = TABS[(idx + 1) % TABS.length] ?? tab;
    } else if (e.key === "ArrowLeft") {
      next = TABS[(idx - 1 + TABS.length) % TABS.length] ?? tab;
    } else if (e.key === "Home") {
      next = TABS[0] ?? tab;
    } else if (e.key === "End") {
      next = TABS[TABS.length - 1] ?? tab;
    }
    if (next) {
      e.preventDefault();
      onTabChange(next);
      const btn = document.querySelector<HTMLButtonElement>(`[role="tab"][data-tab="${next}"]`);
      btn?.focus();
    }
  }

  if (!openFor) return null;

  const githubUrl = githubUrlFor(openFor, repoInfo);

  return (
    <aside
      className="w-[360px] border-l border-border bg-bg flex flex-col animate-slide-in motion-reduce:animate-none"
      aria-label="Node detail panel"
    >
      <header className="flex items-center px-4 py-3 border-b border-border gap-2">
        <Kind kind={openFor.kind} variant="badge" aria-hidden="true" />
        <h2 className="m-0 flex-1 text-[12.5px] font-medium break-all font-sans text-fg">
          {openFor.id}
        </h2>
        <button
          className="bg-transparent border-0 text-fg-muted text-lg leading-none cursor-pointer w-6 h-6 inline-flex items-center justify-center rounded-sm hover:text-fg hover:bg-bg-elev2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
          type="button"
          aria-label="Close detail panel"
          onClick={onClose}
        >
          ×
        </button>
      </header>
      <div
        className="flex border-b border-border bg-bg px-3 gap-4"
        role="tablist"
        aria-label="Detail views"
        onKeyDown={onTabKeyDown}
      >
        {TABS.map((t) => (
          <button
            key={t}
            id={`tab-${t}`}
            role="tab"
            type="button"
            data-tab={t}
            aria-selected={tab === t}
            aria-controls="panel-body"
            tabIndex={tab === t ? 0 : -1}
            onClick={() => onTabChange(t)}
            className="bg-transparent border-0 text-fg-muted py-3 px-0.5 cursor-pointer text-[12.5px] border-b-2 border-b-transparent -mb-px hover:text-fg aria-selected:text-fg aria-selected:border-b-fg aria-selected:font-medium focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:-outline-offset-2"
          >
            {t.charAt(0).toUpperCase() + t.slice(1)}
          </button>
        ))}
      </div>
      <section
        id="panel-body"
        className="flex-1 overflow-y-auto p-4 text-[12.5px]"
        role="tabpanel"
        aria-labelledby={`tab-${tab}`}
        tabIndex={0}
      >
        {tab === "details" && renderDetails(state, githubUrl)}
        {tab === "triggers" && renderTriggers(state)}
        {tab === "impact" && renderImpact(state)}
        {tab === "trace" && renderTrace(state)}
      </section>
    </aside>
  );
}

function renderDetails(state: State, githubUrl: string | null) {
  if (state.detailsError) {
    return <Status type="error">Error: {state.detailsError}</Status>;
  }
  if (state.details === undefined) {
    return <Status type="loading" />;
  }
  if (state.details === null) {
    return <Status type="empty">Not found</Status>;
  }
  const n = state.details;
  return (
    <>
      <Field label="ID" mono>
        {n.id}
      </Field>
      <Field label="Kind">
        <Kind kind={n.kind} variant="pill" />
      </Field>
      <Field label="Label">{n.label}</Field>
      {n.file && (
        <Field label="File" mono>
          {n.file}
        </Field>
      )}
      {n.summary && <Field label="Summary">{n.summary}</Field>}
      {githubUrl && (
        <Field label="GitHub">
          <a
            className="text-accent no-underline hover:underline focus-visible:outline focus-visible:outline-2 focus-visible:outline-accent focus-visible:outline-offset-2 focus-visible:rounded-sm"
            href={githubUrl}
            target="_blank"
            rel="noopener noreferrer"
            aria-label="Open in GitHub"
          >
            Open in GitHub ↗
          </a>
        </Field>
      )}
    </>
  );
}

function renderTriggers(state: State) {
  if (state.detailsError) {
    return <Status type="error">Error: {state.detailsError}</Status>;
  }
  if (state.details === undefined) {
    return <Status type="loading" />;
  }
  if (state.details === null || state.details.entry_triggers.length === 0) {
    return (
      <Status type="empty">No entry triggers — this node is reusable-only or not a workflow</Status>
    );
  }
  return (
    <Field label="Entry triggers">
      <ChipList>
        {state.details.entry_triggers.map((t) => (
          <Chip key={t}>{t}</Chip>
        ))}
      </ChipList>
    </Field>
  );
}

function renderImpact(state: State) {
  if (state.impactError) {
    return <Status type="error">Error: {state.impactError}</Status>;
  }
  if (state.impact === undefined) {
    return <Status type="loading" />;
  }
  if (state.impact === null) {
    return <Status type="empty">Not found</Status>;
  }
  const data = state.impact;
  return (
    <>
      <Field label={`Impacted workflows (${data.workflows.length})`}>
        <FieldRows>
          {data.workflows.length === 0 ? (
            <Status type="empty">(none)</Status>
          ) : (
            data.workflows.map((w) => (
              <FieldValue key={w} mono>
                {w}
              </FieldValue>
            ))
          )}
        </FieldRows>
      </Field>
      <Field label={`Impacted actions (${data.actions.length})`}>
        <FieldRows>
          {data.actions.length === 0 ? (
            <Status type="empty">(none)</Status>
          ) : (
            data.actions.map((a) => (
              <FieldValue key={a.id} mono>
                {a.id} <Chip>{a.kind}</Chip>
              </FieldValue>
            ))
          )}
        </FieldRows>
      </Field>
      {data.unknowns.length > 0 && (
        <Field label="Unknown paths">
          <FieldRows>
            {data.unknowns.map((u) => (
              <FieldValue key={u} mono>
                {u}
              </FieldValue>
            ))}
          </FieldRows>
        </Field>
      )}
    </>
  );
}

function renderTrace(state: State) {
  if (state.traceError) {
    return <Status type="error">Error: {state.traceError}</Status>;
  }
  if (state.trace === undefined) {
    return <Status type="loading" />;
  }
  if (state.trace === null) {
    return <Status type="empty">No entry trigger for this node</Status>;
  }
  return (
    <>
      <Field label="Event used">
        <Chip>{state.trace.event_used}</Chip>
      </Field>
      <Field label="Tree">
        <pre className="font-mono text-xs whitespace-pre overflow-x-auto bg-bg-elev p-3 border border-border-soft rounded-md text-fg">
          {renderTraceTree(state.trace.tree)}
        </pre>
      </Field>
    </>
  );
}
