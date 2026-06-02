import { useCallback, useEffect, useMemo, useState } from "react";
import {
  fetchAllFindings,
  fetchEventImpact,
  fetchGraph,
  fetchRepo,
  fetchSearch,
  fetchTriggers,
} from "./lib/api.ts";
import { getRavelactRf } from "./lib/dev-globals.ts";
import type { FindingFacets } from "./lib/graph-filter.ts";
import { NODE_KINDS } from "./lib/kind-format.ts";
import { readPersistedWidth, writePersistedWidth } from "./lib/panel-width.ts";
import { useDebounced } from "./ui/hooks/useDebounced.ts";
import type {
  FindingWithNode,
  GraphPayload,
  NodeKind,
  RepoInfo,
  TriggerSummary,
} from "./lib/types.ts";
import { ErrorBanner } from "./ui/components/ErrorBanner.tsx";
import { FindingsFloat } from "./ui/components/FindingsFloat.tsx";
import { Graph } from "./ui/components/Graph.tsx";
import { Header } from "./ui/components/Header.tsx";
import { Panel, type Tab } from "./ui/components/Panel.tsx";
import { PoweredBy } from "./ui/components/PoweredBy.tsx";

const NO_FACETS: FindingFacets = { severities: null, sources: null, contexts: null };

function isNodeKind(k: string): k is NodeKind {
  return (NODE_KINDS as ReadonlyArray<string>).includes(k);
}

export function App() {
  const [payload, setPayload] = useState<GraphPayload | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<{ id: string; kind: NodeKind } | null>(null);

  // Active right-panel tab. Lifted out of Panel so it survives
  // node-to-node selection changes. Per-node data slices still reset
  // because <Panel> is keyed on `selected.id` and remounts on change.
  // Reset to "details" inside handleClearSelected so the next opened
  // panel starts on Details.
  const [panelTab, setPanelTab] = useState<Tab>("details");

  // Lifted from Header so the FindingsFloat can render the same data
  // without a second /api/triggers round-trip.
  const [triggers, setTriggers] = useState<TriggerSummary[] | null>(null);

  // Cross-cutting findings list backing the FindingsFloat's Findings tab.
  // Empty when browse ran without `--findings` (the RPC never 404s), so the
  // float simply shows no findings rows in that case.
  const [allFindings, setAllFindings] = useState<FindingWithNode[]>([]);

  // GitHub provenance of the local `--root` repo. `null` when the
  // backend returns 404 (non-git / non-github / detached HEAD), in which
  // case Panel hides the Open-in-GitHub link for local nodes. External
  // nodes still get a link constructed from their id alone.
  const [repoInfo, setRepoInfo] = useState<RepoInfo | null>(null);

  // Search state (server-driven matchedIds).
  const [searchQuery, setSearchQuery] = useState<string>("");
  const debouncedQuery = useDebounced(searchQuery, 120);
  const [matchedIds, setMatchedIds] = useState<Set<string> | null>(null);

  // Event-impact state (analysisIds is the orthogonal filter that
  // the FindingsFloat's event clicks drive).
  const [selectedEvent, setSelectedEvent] = useState<string | null>(null);
  const [analysisIds, setAnalysisIds] = useState<Set<string> | null>(null);

  // Width for the node detail pane. Persisted to localStorage from the
  // handler (not an effect) so the initial mount does not re-write the value
  // that was just read, and storage-blocked environments do not warn on every
  // page load. The right pane is now node-only (the cross-cutting overview
  // moved to FindingsFloat), so this width belongs solely to Panel.
  const [panelWidth, setPanelWidth] = useState<number>(() => readPersistedWidth());
  const handlePanelWidthChange = useCallback((next: number) => {
    setPanelWidth(next);
    writePersistedWidth(next);
  }, []);

  // Findings overlay facets (severity / source / context). Inactive by
  // default; only meaningful when the graph carries findings.
  const [findingFacets, setFindingFacets] = useState<FindingFacets>(NO_FACETS);

  // Whether the graph carries any findings (browse ran with `--findings`)
  // plus the distinct sources present. Drives the Findings tab + filter UI.
  // A findings-free session yields { hasFindings: false, sources: [] }, so
  // every findings affordance stays hidden and the UI is unchanged.
  const findingsMeta = useMemo(() => {
    let hasFindings = false;
    const sources = new Set<string>();
    for (const n of payload?.nodes ?? []) {
      const fc = n.data?.findingCounts;
      if (fc && fc.total > 0) hasFindings = true;
      for (const s of n.data?.findingSources ?? []) sources.add(s);
    }
    return { hasFindings, sources: [...sources].sort() };
  }, [payload]);

  useEffect(() => {
    let cancelled = false;
    fetchGraph()
      .then((g) => {
        if (cancelled) return;
        setPayload(g);
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setError(e instanceof Error ? e.message : String(e));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    fetchTriggers()
      .then((r) => {
        if (cancelled || !r) return;
        setTriggers(r.rows);
      })
      .catch(() => {
        // best-effort: stats strip + overview stay empty on failure
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    fetchRepo()
      .then((r) => {
        if (!cancelled) setRepoInfo(r);
      })
      .catch(() => {
        // best-effort: link is just hidden on failure
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Cross-cutting findings for the float. One fetch on mount; the RPC returns
  // an empty list (never 404) when browse ran without `--findings`, so the
  // float just renders no findings in that case.
  useEffect(() => {
    let cancelled = false;
    fetchAllFindings()
      .then((r) => {
        if (!cancelled) setAllFindings(r.findings);
      })
      .catch(() => {
        // best-effort: the Findings tab stays empty on failure
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Search fetch: debounce + AbortController so stale responses
  // cannot overwrite fresh ones.
  useEffect(() => {
    const trimmed = debouncedQuery.trim();
    if (trimmed === "") {
      setMatchedIds(null);
      return;
    }
    const controller = new AbortController();
    fetchSearch(trimmed, controller.signal)
      .then((r) => {
        if (controller.signal.aborted) return;
        setMatchedIds(new Set(r.matches.map((m) => m.id)));
      })
      .catch((e: unknown) => {
        if (controller.signal.aborted) return;
        if (e instanceof DOMException && e.name === "AbortError") return;
        console.warn("search failed", e);
      });
    return () => controller.abort();
  }, [debouncedQuery]);

  // Event-impact fetch: each click is one fetch; AbortController
  // cancels the previous in case the user clicks rapidly between
  // events.
  useEffect(() => {
    if (selectedEvent === null) {
      setAnalysisIds(null);
      return;
    }
    const controller = new AbortController();
    fetchEventImpact(selectedEvent, controller.signal)
      .then((r) => {
        if (controller.signal.aborted) return;
        setAnalysisIds(new Set(r.nodeIds));
      })
      .catch((e: unknown) => {
        if (controller.signal.aborted) return;
        if (e instanceof DOMException && e.name === "AbortError") return;
        console.warn("event-impact failed", e);
      });
    return () => controller.abort();
  }, [selectedEvent]);

  const handleNodeClick = useCallback((id: string, kind: string) => {
    if (!isNodeKind(kind)) return;
    setSelected({ id, kind });
  }, []);

  // Shared by Graph background-tap and Panel close. Stabilised via
  // useCallback so Graph's `__ravelactRf` install effect (deps: rf
  // only) and Panel's keydown effect (deps: [openFor, onClose]) do not
  // re-run on every App re-render.
  const handleClearSelected = useCallback(() => {
    setSelected(null);
    setPanelTab("details");
  }, []);

  const handleSearchEnter = useCallback(() => {
    const ids = matchedIds ? Array.from(matchedIds) : [];
    getRavelactRf()?.fitNodes(ids);
  }, [matchedIds]);

  // Cross-cutting finding click: open the owning node's Panel on its Findings
  // tab and fit the graph to it (mirrors handleSearchEnter's fitNodes use).
  const handleSelectFinding = useCallback((id: string, kind: NodeKind) => {
    setSelected({ id, kind });
    setPanelTab("findings");
    getRavelactRf()?.fitNodes([id]);
  }, []);

  const nodeCount = payload?.nodes.length ?? 0;

  return (
    <>
      <Header
        nodeCount={nodeCount}
        triggers={triggers}
        searchQuery={searchQuery}
        onSearchChange={setSearchQuery}
        onSearchEnter={handleSearchEnter}
      />
      <main className="flex h-[calc(100%-48px)] relative">
        <section className="flex-1 relative bg-bg min-w-0" data-testid="graph-section">
          {payload && (
            <Graph
              payload={payload}
              onNodeClick={handleNodeClick}
              onBackgroundTap={handleClearSelected}
              onLayoutError={setError}
              selectedId={selected?.id ?? null}
              matchedIds={matchedIds}
              analysisIds={analysisIds}
              findingFacets={findingFacets}
            />
          )}
          {payload && (
            // Bounded top→bottom band so the float never grows into the
            // bottom-left credit pill. `pointer-events-none` lets clicks fall
            // through the empty area below the panel to the graph; the panel
            // itself re-enables pointer events.
            <div className="absolute top-3 bottom-14 left-3 z-10 w-[300px] max-w-[calc(100%-24px)] pointer-events-none">
              <FindingsFloat
                hasFindings={findingsMeta.hasFindings}
                findings={allFindings}
                availableSources={findingsMeta.sources}
                facets={findingFacets}
                onChangeFacets={setFindingFacets}
                onSelectFinding={handleSelectFinding}
                triggers={triggers}
                selectedEvent={selectedEvent}
                onSelectEvent={setSelectedEvent}
              />
            </div>
          )}
          <ErrorBanner message={error} />
          <PoweredBy />
        </section>
        {/* Right pane is node-only: the cross-cutting overview lives in the
            top-left FindingsFloat now, so nothing renders here without a
            selection. */}
        {selected && (
          <Panel
            key={selected.id}
            openFor={selected}
            onClose={handleClearSelected}
            repoInfo={repoInfo}
            tab={panelTab}
            onTabChange={setPanelTab}
            selectedEvent={selectedEvent}
            onSelectEvent={setSelectedEvent}
            width={panelWidth}
            onWidthChange={handlePanelWidthChange}
            hasFindings={findingsMeta.hasFindings}
          />
        )}
      </main>
    </>
  );
}
