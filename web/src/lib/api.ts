import { Code, ConnectError, createClient } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";
import { BrowseService } from "../proto/ravelact/browse/v1/browse_pb.ts";
import type {
  GetEventImpactResponse,
  GetFindingsResponse,
  GetGraphResponse,
  GetImpactResponse,
  GetNodeResponse,
  GetRepoResponse,
  ListFindingsResponse,
  ListTriggersResponse,
  SearchResponse,
  TraceResponse,
} from "../proto/ravelact/browse/v1/browse_pb.ts";
import type { NodeKind, NodeResponseKind } from "./types.ts";

// Trust boundary note: the ConnectRPC server is served by the same
// ravelact binary embedded into this SPA's process group, bound to
// 127.0.0.1 with no other writers, and consumed only by a single local
// user. The generated client + protobuf schema replace the previous
// hand-rolled `fetch("/api/...")` JSON helpers; types now flow from
// `web/src/proto/`.
//
// Connect-Web reuses `location.origin`; the server is mounted at the
// same origin under `/ravelact.browse.v1.BrowseService/<Method>`.
const transport = createConnectTransport({
  baseUrl: typeof location !== "undefined" ? location.origin : "",
});

const client = createClient(BrowseService, transport);

// Connect cancellation surfaces as a thrown `ConnectError` with `code:
// Canceled`. Re-throw as a `DOMException("AbortError")` so existing
// `controller.signal.aborted` branches in `App.tsx` keep working
// without changes.
function rethrowCancelAsAbort(err: unknown): never {
  if (err instanceof ConnectError && err.code === Code.Canceled) {
    throw new DOMException("Aborted", "AbortError");
  }
  throw err;
}

// HTTP 404 → `null` was the JSON-era contract for the 5 nullable
// helpers `fetchTriggers`, `fetchRepo`, `fetchNode`, `fetchImpact`,
// `fetchTrace`. Connect signals "not found" as `ConnectError(code:
// NotFound)`; this wrapper normalises those into `null` so the SPA's
// optional-chaining call sites stay intact. Other Connect errors
// propagate as throws (matching the previous `.ok` checks). Helpers
// that never returned null (`fetchGraph`, `fetchSearch`,
// `fetchEventImpact`) must NOT use this wrapper — those should surface
// transport / server failures as real errors instead of silent
// `null`s.
async function nullOnNotFound<T>(promise: Promise<T>): Promise<T | null> {
  try {
    return await promise;
  } catch (err) {
    if (err instanceof ConnectError && err.code === Code.NotFound) {
      return null;
    }
    rethrowCancelAsAbort(err);
  }
}

export async function fetchGraph(): Promise<GetGraphResponse> {
  return client.getGraph({});
}

export function fetchTriggers(): Promise<ListTriggersResponse | null> {
  return nullOnNotFound(client.listTriggers({}));
}

export function fetchRepo(): Promise<GetRepoResponse | null> {
  return nullOnNotFound(client.getRepo({}));
}

export async function fetchNode(kind: NodeKind, id: string): Promise<GetNodeResponse | null> {
  const resp = await nullOnNotFound(client.getNode({ kind, id }));
  if (resp === null) return null;
  // The proto enum is the full `NodeKind` union, but the server only
  // ever returns `workflow` / `local-action` / `external-action`
  // (other kinds yield NotFound). Narrow at the boundary so consumers
  // can rely on the tighter `NodeResponseKind` TS type without an
  // unchecked cast.
  if (resp.kind !== "workflow" && resp.kind !== "local-action" && resp.kind !== "external-action") {
    throw new Error(`unexpected GetNode kind: ${resp.kind}`);
  }
  return resp;
}

export function fetchImpact(id: string): Promise<GetImpactResponse | null> {
  return nullOnNotFound(client.getImpact({ id }));
}

export function fetchTrace(id: string): Promise<TraceResponse | null> {
  return nullOnNotFound(client.trace({ id }));
}

// Per-node findings for the Findings tab. Never NotFound: the server
// returns an empty `findings` list for unknown ids / kinds and for nodes
// that carry none (and always-empty when browse ran without `--findings`).
export function fetchFindings(kind: NodeKind, id: string): Promise<GetFindingsResponse> {
  return client.getFindings({ kind, id });
}

// Cross-cutting findings list for the FindingsFloat. Never NotFound: the
// server returns an empty list when browse ran without `--findings` (mirrors
// `fetchFindings`). Each row carries the source tool + its graph node id/kind
// so the float can select + fit the node on click.
export function fetchAllFindings(): Promise<ListFindingsResponse> {
  return client.listFindings({});
}

// `signal` lets callers cancel a stale request when the user keeps
// typing — see `App.tsx` for the AbortController orchestration.
export async function fetchSearch(q: string, signal?: AbortSignal): Promise<SearchResponse> {
  try {
    return await client.search({ q }, { signal });
  } catch (err) {
    rethrowCancelAsAbort(err);
  }
}

export async function fetchEventImpact(
  event: string,
  signal?: AbortSignal,
): Promise<GetEventImpactResponse> {
  try {
    return await client.getEventImpact({ event }, { signal });
  } catch (err) {
    rethrowCancelAsAbort(err);
  }
}

// Re-exported helper used by `fetchNode`'s narrowing logic above. Kept
// inline rather than in `types.ts` so the narrowing rule lives next to
// the call site that enforces it.
export type { NodeResponseKind };
