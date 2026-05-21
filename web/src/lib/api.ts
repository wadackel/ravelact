import type {
  EventImpactResponse,
  GraphPayload,
  ImpactResponse,
  NodeKind,
  NodeResponse,
  RepoInfo,
  SearchResponse,
  TraceResponse,
  TriggersResponse,
} from "./types.ts";

// Trust boundary note: every `/api/*` endpoint is served by the ravelact
// binary embedded into the same process group as this SPA, bound to
// `127.0.0.1` with no other writers, and consumed only by a single local
// user. We therefore parse JSON responses with a structural `as T` cast
// instead of a runtime schema validator (zod / valibot). If the binary's
// response shape drifts from `types.ts` the TypeScript type system will
// surface the mismatch at the call-site rather than crashing here.

async function fetchJsonOrNull<T>(path: string): Promise<T | null> {
  const r = await fetch(path);
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`${path} ${r.status}`);
  return (await r.json()) as T;
}

export async function fetchGraph(): Promise<GraphPayload> {
  const r = await fetch("/api/graph");
  if (!r.ok) throw new Error(`/api/graph ${r.status}`);
  return (await r.json()) as GraphPayload;
}

export function fetchTriggers(): Promise<TriggersResponse | null> {
  return fetchJsonOrNull<TriggersResponse>("/api/triggers");
}

export function fetchRepo(): Promise<RepoInfo | null> {
  return fetchJsonOrNull<RepoInfo>("/api/repo");
}

export function fetchNode(kind: NodeKind, id: string): Promise<NodeResponse | null> {
  return fetchJsonOrNull<NodeResponse>(
    `/api/node?kind=${encodeURIComponent(kind)}&id=${encodeURIComponent(id)}`,
  );
}

export function fetchImpact(id: string): Promise<ImpactResponse | null> {
  return fetchJsonOrNull<ImpactResponse>(`/api/impact?id=${encodeURIComponent(id)}`);
}

export function fetchTrace(id: string): Promise<TraceResponse | null> {
  return fetchJsonOrNull<TraceResponse>(`/api/trace?id=${encodeURIComponent(id)}`);
}

// `signal` lets callers cancel a stale request when the user keeps
// typing — see `App.tsx` for the AbortController orchestration.
export async function fetchSearch(q: string, signal?: AbortSignal): Promise<SearchResponse> {
  const r = await fetch(`/api/search?q=${encodeURIComponent(q)}`, { signal });
  if (!r.ok) throw new Error(`/api/search ${r.status}`);
  return (await r.json()) as SearchResponse;
}

export async function fetchEventImpact(
  event: string,
  signal?: AbortSignal,
): Promise<EventImpactResponse> {
  const r = await fetch(`/api/event-impact?event=${encodeURIComponent(event)}`, { signal });
  if (!r.ok) throw new Error(`/api/event-impact ${r.status}`);
  return (await r.json()) as EventImpactResponse;
}
