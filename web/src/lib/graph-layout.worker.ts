/// <reference lib="WebWorker" />
// Module Worker entry. Vite resolves the `?worker` import in
// `graph-layout.ts` to a constructor that spawns this file in a separate
// thread. Keep this file tiny — message glue only — so the worker chunk
// stays close to "dagre + the sync layout function" in size.
//
// Wire protocol (D-transferable): inbound message data is an
// `ArrayBuffer` containing UTF-8 JSON of `GraphPayload`. Outbound success
// is `{ok:true, resultBuffer: ArrayBuffer}` where the buffer holds the
// UTF-8 JSON of `LayoutResult`. Both directions use `postMessage`'s
// transfer list so the underlying memory is moved (no structuredClone).
//
// Trust boundary: the inbound ArrayBuffer is `postMessage`-d only by
// `graph-layout.ts:runInWorker`, which sits in the same same-origin
// SPA bundle. The payload itself originates from `/api/graph` —
// served by the local `ravelact` process bound to `127.0.0.1`, i.e.
// the operator's own machine — so `JSON.parse` here is processing
// data from a trusted local source, not arbitrary network input.

import { computeLayoutSync, type WorkerResponse } from "./graph-layout-core.ts";
import type { GraphPayload } from "./types.ts";

declare const self: DedicatedWorkerGlobalScope;

self.addEventListener("message", (ev: MessageEvent<ArrayBuffer>) => {
  let payload: GraphPayload;
  try {
    const text = new TextDecoder().decode(ev.data);
    payload = JSON.parse(text) as GraphPayload;
  } catch (err) {
    const response: WorkerResponse = {
      ok: false,
      error: `worker: failed to decode payload: ${
        err instanceof Error ? err.message : String(err)
      }`,
    };
    self.postMessage(response);
    return;
  }
  try {
    const result = computeLayoutSync(payload);
    const encoded = new TextEncoder().encode(JSON.stringify(result));
    const buffer =
      encoded.byteOffset === 0 && encoded.byteLength === encoded.buffer.byteLength
        ? (encoded.buffer as ArrayBuffer)
        : encoded.buffer.slice(encoded.byteOffset, encoded.byteOffset + encoded.byteLength);
    const response: WorkerResponse = { ok: true, resultBuffer: buffer };
    self.postMessage(response, [buffer]);
  } catch (err) {
    const response: WorkerResponse = {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    };
    self.postMessage(response);
  }
});
