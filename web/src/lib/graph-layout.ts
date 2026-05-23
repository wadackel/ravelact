import type { GraphPayload } from "./types.ts";
import { computeLayoutSync, type LayoutResult, type WorkerResponse } from "./graph-layout-core.ts";
import GraphLayoutWorker from "./graph-layout.worker.ts?worker";

// Re-export the core surface so existing consumers can continue to
// import from `./graph-layout.ts` without learning about the internal
// split (graph-layout-core.ts holds the dagre-running sync function;
// this module wires the Worker around it).
export { computeLayoutSync } from "./graph-layout-core.ts";
export type { LayoutResult, WorkerResponse } from "./graph-layout-core.ts";

/**
 * Async wrapper. Three failure paths, all leading to a usable graph
 * unless the payload itself is malformed:
 *
 *   1. `globalThis.Worker` undefined (jsdom / very old browser / CSP):
 *      run synchronously on the main thread.
 *   2. Worker construction throws or the worker emits an `error` event
 *      BEFORE the first message handler has fired (chunk 404 / module
 *      load failure): retry once on the main thread.
 *   3. In-worker exception caught and posted back as `{ok:false,
 *      error}`, OR an `error` event AFTER message exchange has begun:
 *      reject the Promise so Graph.tsx can surface it through
 *      `onLayoutError` → ErrorBanner. Re-running on main thread would
 *      hit the same exception, so no retry.
 */
export function computeLayout(payload: GraphPayload, signal?: AbortSignal): Promise<LayoutResult> {
  if (typeof Worker === "undefined") {
    try {
      return Promise.resolve(computeLayoutSync(payload));
    } catch (e) {
      return Promise.reject(e);
    }
  }
  return runInWorker(payload, signal).catch((err) => {
    // If the caller aborted (component unmounted / payload changed
    // mid-flight) the error is an `AbortError`; do not silently fall
    // back to the main thread or the abort would be wasted work.
    if (signal?.aborted) throw err;
    if (err instanceof WorkerStartupError) {
      return computeLayoutSync(payload);
    }
    throw err;
  });
}

class WorkerStartupError extends Error {
  constructor(cause: string) {
    super(`worker startup failed: ${cause}`);
    this.name = "WorkerStartupError";
  }
}

function runInWorker(payload: GraphPayload, signal?: AbortSignal): Promise<LayoutResult> {
  return new Promise<LayoutResult>((resolve, reject) => {
    if (signal?.aborted) {
      reject(new DOMException("computeLayout aborted before start", "AbortError"));
      return;
    }
    let worker: Worker;
    try {
      worker = new GraphLayoutWorker();
    } catch (e) {
      reject(new WorkerStartupError(String(e)));
      return;
    }

    // `error` events fire for both (a) module load failures and (b)
    // uncaught synchronous exceptions inside the worker script. In our
    // worker, all post-startup logic is wrapped in try/catch and
    // surfaced via the `{ok:false, error}` envelope, so (b) should
    // never actually reach `onerror`. Tracking whether a message has
    // ever round-tripped lets us classify a late `error` event as a
    // runtime failure (don't retry) rather than a startup failure
    // (retry once on main thread), in case a future code path leaks an
    // unhandled exception past the worker's try/catch.
    let messageEverFired = false;

    const cleanup = () => {
      worker.removeEventListener("message", onMessage);
      worker.removeEventListener("error", onError);
      signal?.removeEventListener("abort", onAbort);
      worker.terminate();
    };

    const onMessage = (ev: MessageEvent<WorkerResponse>) => {
      messageEverFired = true;
      cleanup();
      if (ev.data.ok) {
        try {
          const text = new TextDecoder().decode(ev.data.resultBuffer);
          resolve(JSON.parse(text) as LayoutResult);
        } catch (e) {
          reject(new Error(`failed to decode worker result: ${String(e)}`));
        }
      } else {
        reject(new Error(ev.data.error));
      }
    };
    const onError = (ev: ErrorEvent) => {
      cleanup();
      if (messageEverFired) {
        // Runtime failure after a successful round-trip — do not retry.
        reject(new Error(ev.message || "worker runtime error"));
      } else {
        reject(new WorkerStartupError(ev.message || "worker error event"));
      }
    };
    // When the consumer (Graph.tsx's useEffect) aborts mid-flight —
    // typically because the component unmounted or `payload` changed —
    // tear down the worker so it does not keep computing past the
    // owner's lifetime. At 5k-node scale dagre runs for ~100 s; an
    // orphan worker would pin a CPU core for that whole window.
    const onAbort = () => {
      cleanup();
      reject(new DOMException("computeLayout aborted", "AbortError"));
    };
    worker.addEventListener("message", onMessage);
    worker.addEventListener("error", onError);
    signal?.addEventListener("abort", onAbort);
    // D-transferable: stringify + encode to ArrayBuffer, then transfer
    // the underlying buffer to the worker. structuredClone of the
    // unencoded payload would otherwise dominate the round-trip at
    // multi-thousand-node scale.
    const encoded = new TextEncoder().encode(JSON.stringify(payload));
    // `encoded.buffer` is the freshly-allocated ArrayBuffer backing the
    // Uint8Array — V8/SpiderMonkey/JSC all match `byteLength` here, but
    // we explicitly slice into the exact range to insulate against any
    // future engine returning an oversized buffer (per the TextEncoder
    // spec, the backing buffer's size is implementation-defined as long
    // as the contents are correct in [byteOffset, byteOffset+byteLength)).
    const buffer =
      encoded.byteOffset === 0 && encoded.byteLength === encoded.buffer.byteLength
        ? (encoded.buffer as ArrayBuffer)
        : encoded.buffer.slice(encoded.byteOffset, encoded.byteOffset + encoded.byteLength);
    worker.postMessage(buffer, [buffer]);
  });
}
