import { useCallback, useEffect, useState } from "react";
import { fetchEventImpact, fetchGraph, fetchRepo, fetchSearch, fetchTriggers } from "./lib/api.ts";
import { getRavelactRf } from "./lib/dev-globals.ts";
import { NODE_KINDS } from "./lib/kind-format.ts";
import { useDebounced } from "./ui/hooks/useDebounced.ts";
import type { GraphPayload, NodeKind, RepoInfo, TriggerSummary } from "./lib/types.ts";
import { ErrorBanner } from "./ui/components/ErrorBanner.tsx";
import { Graph } from "./ui/components/Graph.tsx";
import { Header } from "./ui/components/Header.tsx";
import { OverviewPane } from "./ui/components/OverviewPane.tsx";
import { Panel, type Tab } from "./ui/components/Panel.tsx";

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

  // Lifted from Header so the OverviewPane can render the same data
  // without a second /api/triggers round-trip.
  const [triggers, setTriggers] = useState<TriggerSummary[] | null>(null);

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
  // OverviewPane's clicks drive).
  const [selectedEvent, setSelectedEvent] = useState<string | null>(null);
  const [analysisIds, setAnalysisIds] = useState<Set<string> | null>(null);

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
        setAnalysisIds(new Set(r.node_ids));
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
        <section className="flex-1 relative bg-bg min-w-0">
          {payload && (
            <Graph
              payload={payload}
              onNodeClick={handleNodeClick}
              onBackgroundTap={handleClearSelected}
              onLayoutError={setError}
              selectedId={selected?.id ?? null}
              matchedIds={matchedIds}
              analysisIds={analysisIds}
            />
          )}
          <ErrorBanner message={error} />
        </section>
        {selected ? (
          <Panel
            key={selected.id}
            openFor={selected}
            onClose={handleClearSelected}
            repoInfo={repoInfo}
            tab={panelTab}
            onTabChange={setPanelTab}
            selectedEvent={selectedEvent}
            onSelectEvent={setSelectedEvent}
          />
        ) : (
          <OverviewPane
            triggers={triggers}
            selectedEvent={selectedEvent}
            onSelectEvent={setSelectedEvent}
          />
        )}
      </main>
    </>
  );
}
