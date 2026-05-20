#!/usr/bin/env -S deno run --allow-all
/**
 * script/perf-check-browse.ts — `ravelact browse` performance harness.
 *
 * Measures the React 19 + ReactFlow (@xyflow/react) SPA at two scales:
 *   - dogfood   : the host repo (~16 nodes, ~42 edges)
 *   - synthetic : 300 generated workflows (≒ 871 elements)
 *
 * Output:
 *   .wadackel/qa/<YYYY-MM-DD_HH-MM>_browse-perf-300/
 *     ├── report.md            (metric tables + methodology + screenshots list)
 *     ├── recording.webm       (one continuous recording for the whole session)
 *     └── screenshots/         (per-scale before/after captures)
 *
 * Prerequisites:
 *   - Release binary at ./target/release/ravelact (run `nix develop -c just build-release`)
 *   - agent-browser state file at ~/.agent-browser-state/main.json (run `ab-state-refresh`)
 *   - Deno on PATH (not yet wired into flake.nix dev shell)
 *
 * Plan: ~/.claude/plans/20260517T2107-browse-perf-check-300-workflows.md
 */

import { dirname, fromFileUrl, join, resolve } from "jsr:@std/path@1";

// ---------------------------------------------------------------------------
// Synthetic estate generation
// ---------------------------------------------------------------------------

/**
 * Hand-maintained mirror of `write_synthetic_estate` in
 * tests/e2e_browse.rs (lines 21-41). If you change the Rust helper,
 * update this function too. The integration test
 * `synthetic_estate_generates_300_workflows` exists to catch the count
 * end of the contract; Task 1 also cross-checks via `ravelact dump | jq`.
 */
function writeSyntheticEstate(dir: string, workflows: number): void {
  const wfDir = join(dir, ".github", "workflows");
  Deno.mkdirSync(wfDir, { recursive: true });
  const reusableCount = Math.min(workflows, 30);
  for (let i = 0; i < workflows; i++) {
    const idx = String(i).padStart(3, "0");
    const path = join(wfDir, `wf-${idx}.yaml`);
    const content = i < reusableCount
      ? `name: Reusable ${i}\non:\n  workflow_call:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: echo wf-${i}\n`
      : `name: Caller ${i}\non:\n  push:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: echo wf-${i}\n  call:\n    uses: ./.github/workflows/wf-${String(i % reusableCount).padStart(3, "0")}.yaml\n`;
    Deno.writeTextFileSync(path, content);
  }
}

// ---------------------------------------------------------------------------
// ravelact browse spawning (mirror of tests/e2e_browse.rs:61-105 in spirit)
// ---------------------------------------------------------------------------

interface SpawnedBrowse {
  proc: Deno.ChildProcess;
  port: number;
}

async function spawnBrowse(
  binaryPath: string,
  root: string,
  extraArgs: string[] = [],
): Promise<SpawnedBrowse> {
  const proc = new Deno.Command(binaryPath, {
    args: [
      "--root",
      root,
      "browse",
      "--no-open",
      "--port",
      "0",
      ...extraArgs,
    ],
    stdout: "piped",
    stderr: "piped",
  }).spawn();

  // Loop+deadline mirror of tests/e2e_browse.rs: read lines, 15s timeout,
  // EOF-as-fatal (child exited before binding).
  const decoder = new TextDecoder();
  const reader = proc.stdout.getReader();
  const deadline = Date.now() + 15_000;
  let buf = "";
  try {
    while (true) {
      if (Date.now() >= deadline) {
        await terminateChild(proc);
        throw new Error("timed out waiting for ravelact bind announcement");
      }
      const { value, done } = await reader.read();
      if (done) {
        await terminateChild(proc);
        throw new Error(
          "ravelact browse exited before announcing bind (EOF on stdout)",
        );
      }
      buf += decoder.decode(value, { stream: true });
      const newlineIdx = buf.indexOf("\n");
      if (newlineIdx < 0) continue;
      const line = buf.slice(0, newlineIdx);
      buf = buf.slice(newlineIdx + 1);
      const port = parseBindPort(line);
      if (port !== null) {
        return { proc, port };
      }
    }
  } finally {
    // Release the reader without cancelling — the caller still owns the
    // process and may want stdout drained later. cancel() would close it.
    reader.releaseLock();
  }
}

function parseBindPort(line: string): number | null {
  const m = line.match(/http:\/\/127\.0\.0\.1:(\d+)\//);
  if (!m) return null;
  const n = parseInt(m[1], 10);
  return Number.isFinite(n) ? n : null;
}

async function terminateChild(proc: Deno.ChildProcess): Promise<void> {
  try {
    proc.kill("SIGTERM");
  } catch {
    // already gone
  }
  try {
    await proc.status;
  } catch {
    // ignore
  }
}

// ---------------------------------------------------------------------------
// agent-browser thin wrapper
// ---------------------------------------------------------------------------

const AB_SESSION = `claude-${Deno.pid}`;
const AB_STATE = `${Deno.env.get("HOME")}/.agent-browser-state/main.json`;

async function ab(args: string[], opts?: { stateOnFirst?: boolean }): Promise<string> {
  const full = ["--session", AB_SESSION];
  if (opts?.stateOnFirst) full.push("--state", AB_STATE);
  full.push(...args);
  const cmd = new Deno.Command("agent-browser", {
    args: full,
    stdout: "piped",
    stderr: "piped",
  });
  const out = await cmd.output();
  if (!out.success) {
    const err = new TextDecoder().decode(out.stderr);
    throw new Error(`agent-browser ${args.join(" ")} failed: ${err.trim()}`);
  }
  return new TextDecoder().decode(out.stdout);
}

async function evalJson<T>(expr: string): Promise<T> {
  // agent-browser eval returns the value as a quoted JSON string when the
  // expression returns a string, or as raw JSON for other types. Wrapping in
  // JSON.stringify guarantees we always parse a string-encoded JSON.
  const cmd = new Deno.Command("agent-browser", {
    args: ["--session", AB_SESSION, "eval", "--stdin"],
    stdin: "piped",
    stdout: "piped",
    stderr: "piped",
  });
  const child = cmd.spawn();
  const writer = child.stdin.getWriter();
  await writer.write(new TextEncoder().encode(expr));
  await writer.close();
  const out = await child.output();
  if (!out.success) {
    const err = new TextDecoder().decode(out.stderr);
    throw new Error(`eval failed: ${err.trim()}`);
  }
  const raw = new TextDecoder().decode(out.stdout).trim();
  // agent-browser's `eval` JSON-encodes the return value before printing.
  // Our expressions return `JSON.stringify(...)`, so the stdout is a
  // doubly-encoded JSON string (e.g. `"\"{\\\"ready\\\":true}\""`). The
  // first JSON.parse strips the outer wrapper, the second decodes the
  // inner structured value. If the first parse already yields a non-string
  // (e.g. number / object), return it directly.
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error(`eval output was not JSON-parseable: ${raw}`);
  }
  if (typeof parsed === "string") {
    try {
      return JSON.parse(parsed) as T;
    } catch {
      // Inner string is a literal, return as-is (e.g. unused branch).
      return parsed as unknown as T;
    }
  }
  return parsed as T;
}

// ---------------------------------------------------------------------------
// Metric collection
// ---------------------------------------------------------------------------

interface ScaleMetrics {
  label: string;
  workflows: number;
  node_count: number;
  edge_count: number;
  api_graph_bytes: number;
  initial_load_ms: number;
  drag_fps: number;
  settle_ms: number | "capped";
  highlight_latency_ms: { p50: number; p95: number; samples: number };
  heap_initial_bytes: number;
  heap_after_interactions_bytes: number;
  viewport_mutation_event_count: number;
}

interface NavigationResult {
  t_navigation_start: number;
  t_graph_ready: number;
  initial_load_ms: number;
  node_count: number;
  edge_count: number;
}

// ReactFlow renders via CSS transforms + SVG, so there is no canvas
// render-event stream like Cytoscape's `render`. We approximate render
// activity by observing class/style mutations on the viewport element.
// The probe is best-effort: it captures churn during pan/zoom and any
// React-commit-driven style recomputation.
const PERF_PROBES_INJECT = `
  (function(){
    globalThis.__perf = { ready: true };
    globalThis.__perfFps = { frames: 0, start: 0, stop: true };
    globalThis.__perfStart = () => { __perfFps.frames = 0; __perfFps.start = performance.now(); __perfFps.stop = false; const tick = () => { if (__perfFps.stop) return; __perfFps.frames++; requestAnimationFrame(tick); }; requestAnimationFrame(tick); };
    globalThis.__perfStop = () => { __perfFps.stop = true; const dur = performance.now() - __perfFps.start; return dur > 0 ? (__perfFps.frames / dur) * 1000 : 0; };
    globalThis.__perfRender = [];
    const viewport = document.querySelector('.react-flow__viewport');
    if (viewport && !globalThis.__perfMo) {
      const mo = new MutationObserver(() => __perfRender.push(performance.now()));
      mo.observe(viewport, { attributes: true, attributeFilter: ['style', 'transform'], subtree: false });
      globalThis.__perfMo = mo;
    }
    return 'ok';
  })()
`;

async function waitForGraphReady(): Promise<NavigationResult> {
  // Poll for __ravelactRf + node count > 0; record timeOrigin and
  // the first moment all readiness conditions hold. Dagre layout
  // completes synchronously inside the Graph mount effect, so as
  // soon as nodes() is non-empty the layout is already final.
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const res = await evalJson<{ ready: boolean; nodes?: number; edges?: number; timeOrigin?: number; now?: number }>(
      `(function(){
        const rf = globalThis.__ravelactRf;
        if (!rf) return JSON.stringify({ ready: false });
        const nodes = rf.getNodes().length;
        const edges = rf.getEdges().length;
        if (nodes === 0) return JSON.stringify({ ready: false });
        return JSON.stringify({ ready: true, nodes, edges, timeOrigin: performance.timeOrigin, now: performance.now() });
      })()`,
    );
    if (res.ready && res.now !== undefined && res.timeOrigin !== undefined) {
      return {
        t_navigation_start: res.timeOrigin,
        t_graph_ready: res.timeOrigin + res.now,
        initial_load_ms: res.now,
        node_count: res.nodes ?? 0,
        edge_count: res.edges ?? 0,
      };
    }
    await sleep(80);
  }
  throw new Error("timed out waiting for graph ready");
}

async function measureDragFps(): Promise<number> {
  // Start rAF counter, drive a 3-second panBy loop, then sample FPS.
  //
  // The rAF loop is kicked off as fire-and-forget (window-scoped state,
  // no returned Promise) and polled from Deno. Returning a long-lived
  // Promise from `evalJson` is rejected by CDP with "Promise was
  // collected" because the page-context Promise can be GC'd before
  // it resolves when the eval Promise resolution and the rAF loop
  // are interleaved with React renders.
  await evalJson(`(function(){ globalThis.__perfStart(); return 'ok'; })()`);
  await evalJson(`
    (function(){
      globalThis.__panState = { running: true, start: performance.now() };
      const rf = globalThis.__ravelactRf;
      let dx = 0;
      const step = () => {
        const s = globalThis.__panState;
        if (!s || !s.running) return;
        if (performance.now() - s.start > 3000) { s.running = false; return; }
        dx = (dx + 6) % 200;
        rf.panBy(dx % 12 - 6, dx % 8 - 4);
        requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
      return 'ok';
    })()
  `);
  // Poll until the page-side loop signals completion (or a deadline).
  const deadline = Date.now() + 6_000;
  while (Date.now() < deadline) {
    const done = await evalJson<boolean>(
      `(function(){ return JSON.stringify(!globalThis.__panState || !globalThis.__panState.running); })()`,
    );
    if (done) break;
    await sleep(100);
  }
  return await evalJson<number>(
    `(function(){ return JSON.stringify(globalThis.__perfStop()); })()`,
  );
}

async function measureSettleTime(): Promise<number | "capped"> {
  // Trigger a pan, then wait for the viewport-mutation stream to go
  // quiet for 100ms. Ceiling at 5s. Fire-and-forget the rAF watcher
  // so the eval Promise resolves immediately and CDP cannot collect
  // it mid-loop. The result lands in `globalThis.__settleResult` and
  // is polled from Deno.
  await evalJson(`
    (function(){
      globalThis.__settleResult = null;
      const rf = globalThis.__ravelactRf;
      globalThis.__perfRender.length = 0;
      rf.panBy(80, 40);
      const dragEnd = performance.now();
      const ceiling = dragEnd + 5000;
      const tick = () => {
        const now = performance.now();
        if (now >= ceiling) { globalThis.__settleResult = -1; return; }
        const arr = globalThis.__perfRender;
        if (arr.length > 0) {
          const last = arr[arr.length - 1];
          if (now - last >= 100) {
            globalThis.__settleResult = last - dragEnd;
            return;
          }
        } else if (now - dragEnd >= 100) {
          globalThis.__settleResult = 0;
          return;
        }
        requestAnimationFrame(tick);
      };
      requestAnimationFrame(tick);
      return 'ok';
    })()
  `);
  const deadline = Date.now() + 7_000;
  while (Date.now() < deadline) {
    const v = await evalJson<number | null>(
      `(function(){ return JSON.stringify(globalThis.__settleResult); })()`,
    );
    if (v !== null) {
      return v < 0 ? "capped" : v;
    }
    await sleep(80);
  }
  return "capped";
}

interface HighlightSamples {
  p50: number;
  p95: number;
  samples: number;
}

async function measureHighlightLatency(): Promise<HighlightSamples> {
  // Tap up to 20 distinct workflow nodes evenly spaced, measure
  // tap-enter → faded-applied per tap.
  const raw = await evalJson<number[]>(`
    (function(){
      return (async function(){
        const rf = globalThis.__ravelactRf;
        const wfs = rf.getNodes().filter(n => n.data.kind === 'workflow');
        if (wfs.length === 0) return JSON.stringify([]);
        const count = Math.min(20, wfs.length);
        const step = Math.max(1, Math.floor(wfs.length / count));
        const out = [];
        for (let i = 0; i < count; i++) {
          const node = wfs[i * step % wfs.length];
          performance.clearMarks('perf:tap-enter');
          performance.clearMarks('perf:faded-applied');
          performance.clearMeasures('highlight');
          rf.tapNode(node.id);
          // Wait for the React commit → useEffect → mark.
          await new Promise(r => {
            const deadline = performance.now() + 1000;
            const check = () => {
              const entries = performance.getEntriesByName('perf:faded-applied', 'mark');
              if (entries.length > 0) return r('ok');
              if (performance.now() >= deadline) return r('timeout');
              setTimeout(check, 10);
            };
            check();
          });
          try {
            performance.measure('highlight', 'perf:tap-enter', 'perf:faded-applied');
            const m = performance.getEntriesByName('highlight', 'measure');
            if (m.length > 0) out.push(m[0].duration);
          } catch (e) {
            // mark missing → skip this sample
          }
          await new Promise(r => setTimeout(r, 30));
        }
        return JSON.stringify(out);
      })();
    })()
  `);
  if (!raw || raw.length === 0) return { p50: 0, p95: 0, samples: 0 };
  const sorted = [...raw].sort((a, b) => a - b);
  const pct = (p: number) => sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * p))];
  return { p50: pct(0.5), p95: pct(0.95), samples: sorted.length };
}

async function snapshotHeap(): Promise<number> {
  return await evalJson<number>(`
    (function(){
      const m = performance.memory;
      return JSON.stringify(m ? m.usedJSHeapSize : 0);
    })()
  `);
}

async function curlBytes(port: number, path: string): Promise<number> {
  const out = await new Deno.Command("curl", {
    args: ["-sS", "-o", "/dev/null", "-w", "%{size_download}", `http://127.0.0.1:${port}${path}`],
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!out.success) return -1;
  const n = parseInt(new TextDecoder().decode(out.stdout).trim(), 10);
  return Number.isFinite(n) ? n : -1;
}

// ---------------------------------------------------------------------------
// Per-scale runner
// ---------------------------------------------------------------------------

interface MeasureOpts {
  label: string;
  root: string;
  binaryPath: string;
  workflows: number; // -1 for dogfood (no synthetic)
  isFirst: boolean;
  runDir: string;
}

async function measureScale(opts: MeasureOpts): Promise<ScaleMetrics> {
  console.log(`\n=== measuring ${opts.label} (${opts.workflows} synthetic, root=${opts.root}) ===`);
  const { proc, port } = await spawnBrowse(opts.binaryPath, opts.root);
  try {
    await ab(["tab", "new", `http://127.0.0.1:${port}/`], { stateOnFirst: opts.isFirst });
    // Skip the conventional `wait 3000`. We poll for graph-ready directly so
    // the reported initial-load time excludes the fixed wait. Polling uses an
    // 80 ms interval (see waitForGraphReady) and returns as soon as cy +
    // elements are present, giving a tighter initial-load number.
    const nav = await waitForGraphReady();
    await evalJson(PERF_PROBES_INJECT);
    const apiBytes = await curlBytes(port, "/api/graph");
    const heapInitial = await snapshotHeap();
    const dragFps = await measureDragFps();
    const settle = await measureSettleTime();
    const highlight = await measureHighlightLatency();
    const heapAfter = await snapshotHeap();
    const renderCount = await evalJson<number>(`(function(){ return JSON.stringify(globalThis.__perfRender.length); })()`);
    await ab(["screenshot", join(opts.runDir, "screenshots", `qa-perf-${opts.label}.png`)]);
    return {
      label: opts.label,
      workflows: opts.workflows,
      node_count: nav.node_count,
      edge_count: nav.edge_count,
      api_graph_bytes: apiBytes,
      initial_load_ms: nav.initial_load_ms,
      drag_fps: dragFps,
      settle_ms: settle,
      highlight_latency_ms: highlight,
      heap_initial_bytes: heapInitial,
      heap_after_interactions_bytes: heapAfter,
      viewport_mutation_event_count: renderCount,
    };
  } finally {
    await terminateChild(proc);
  }
}

// ---------------------------------------------------------------------------
// Mirror cross-check via `ravelact dump | jq`
// ---------------------------------------------------------------------------

async function verifyMirrorAgainstRust(binaryPath: string, root: string): Promise<{ total: number; reusable: number }> {
  const dump = await new Deno.Command(binaryPath, {
    args: ["--root", root, "dump"],
    stdout: "piped",
    stderr: "piped",
  }).output();
  if (!dump.success) throw new Error("ravelact dump failed");
  const json = JSON.parse(new TextDecoder().decode(dump.stdout));
  const total = Array.isArray(json.workflows) ? json.workflows.length : 0;
  let reusable = 0;
  for (const w of json.workflows ?? []) {
    if (Array.isArray(w.triggers) && w.triggers.some((t: { event?: { kind?: string } }) => t?.event?.kind === "workflow_call")) {
      reusable++;
    }
  }
  return { total, reusable };
}

// ---------------------------------------------------------------------------
// Report writing
// ---------------------------------------------------------------------------

function fmtBytes(n: number): string {
  if (n < 0) return "n/a";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(2)} MB`;
}

function writeReport(args: {
  runDir: string;
  dogfood: ScaleMetrics;
  at300: ScaleMetrics;
  mirrorCheck: { total: number; reusable: number };
  pkgVersions: { xyflow?: string; dagre?: string };
  recordingPath: string;
  screenshotPaths: string[];
}): void {
  const { runDir, dogfood, at300, mirrorCheck, pkgVersions, recordingPath, screenshotPaths } = args;
  const fmtFps = (f: number) => f.toFixed(1);
  const fmtMs = (n: number) => `${n.toFixed(1)} ms`;
  const fmtSettle = (s: number | "capped") => s === "capped" ? "> 5000 ms (capped)" : `${(s as number).toFixed(1)} ms`;

  const body = [
    `# Browse perf report — dogfood vs. 300 workflows`,
    ``,
    `Generated: ${new Date().toISOString()}`,
    ``,
    `## Methodology`,
    ``,
    `- Harness: \`script/perf-check-browse.ts\` (Deno) — see plan \`~/.claude/plans/20260517T2107-browse-perf-check-300-workflows.md\`.`,
    `- Two scales measured back-to-back in the same Chrome instance via agent-browser:`,
    `  1. **dogfood**: \`./target/release/ravelact --root . browse\` (host repo, ~16 nodes).`,
    `  2. **synthetic-300**: TempDir with 300 generated workflows (30 reusable + 270 caller).`,
    `- Synthetic estate generation is **TS file I/O before browser navigation** — its time is NOT included in any "initial load" number reported here.`,
    `- Versions: see \`web/package.json\`. Detected at run-time: @xyflow/react=${pkgVersions.xyflow ?? "unknown"}, dagre=${pkgVersions.dagre ?? "unknown"}.`,
    `- TS↔Rust mirror cross-check (synthetic-300): \`ravelact dump | jq '.workflows | length'\` = ${mirrorCheck.total} (expected 300), reusable count (\`workflow_call\` triggers) = ${mirrorCheck.reusable} (expected 30).`,
    `- "Coarse heap snapshot" — \`performance.memory.usedJSHeapSize\` is bucketed to ~100 KB; small leaks below that resolution are invisible.`,
    `- "Drag FPS" is sampled during scripted \`rf.panBy\` for ≥ 3 s. ReactFlow pans by mutating the viewport's CSS transform, so per-frame cost is style recompute + composite (no canvas redraw).`,
    `- "Settle time" is the duration after a single pan until the viewport-element mutation stream stays quiet ≥ 100 ms; ceiling 5 s. A \`MutationObserver\` on \`.react-flow__viewport\` style/transform attributes is the source.`,
    `- "Highlight latency" is \`performance.measure('highlight', 'perf:tap-enter', 'perf:faded-applied')\` across 20 distinct workflow nodes; p50/p95 reported.`,
    ``,
    `## Results`,
    ``,
    `| Metric | dogfood | synthetic-300 | Δ |`,
    `|---|---|---|---|`,
    `| nodes (rf.getNodes().length) | ${dogfood.node_count} | ${at300.node_count} | ${at300.node_count - dogfood.node_count} |`,
    `| edges (rf.getEdges().length) | ${dogfood.edge_count} | ${at300.edge_count} | ${at300.edge_count - dogfood.edge_count} |`,
    `| /api/graph size | ${fmtBytes(dogfood.api_graph_bytes)} | ${fmtBytes(at300.api_graph_bytes)} | ${(at300.api_graph_bytes - dogfood.api_graph_bytes >= 0 ? "+" : "")}${fmtBytes(at300.api_graph_bytes - dogfood.api_graph_bytes)} |`,
    `| initial load (timeOrigin → first ready) | ${fmtMs(dogfood.initial_load_ms)} | ${fmtMs(at300.initial_load_ms)} | ${(at300.initial_load_ms - dogfood.initial_load_ms).toFixed(1)} ms |`,
    `| drag FPS (3 s sample) | ${fmtFps(dogfood.drag_fps)} | ${fmtFps(at300.drag_fps)} | ${(at300.drag_fps - dogfood.drag_fps).toFixed(1)} fps |`,
    `| settle after pan | ${fmtSettle(dogfood.settle_ms)} | ${fmtSettle(at300.settle_ms)} | — |`,
    `| highlight latency p50 (n samples) | ${fmtMs(dogfood.highlight_latency_ms.p50)} (n=${dogfood.highlight_latency_ms.samples}) | ${fmtMs(at300.highlight_latency_ms.p50)} (n=${at300.highlight_latency_ms.samples}) | — |`,
    `| highlight latency p95 | ${fmtMs(dogfood.highlight_latency_ms.p95)} | ${fmtMs(at300.highlight_latency_ms.p95)} | — |`,
    `| heap initial (coarse) | ${fmtBytes(dogfood.heap_initial_bytes)} | ${fmtBytes(at300.heap_initial_bytes)} | — |`,
    `| heap after 20 taps (coarse) | ${fmtBytes(dogfood.heap_after_interactions_bytes)} | ${fmtBytes(at300.heap_after_interactions_bytes)} | — |`,
    `| viewport mutation events observed | ${dogfood.viewport_mutation_event_count} | ${at300.viewport_mutation_event_count} | — |`,
    ``,
    `## Thresholds (guidance only, not blocking)`,
    ``,
    `- initial load < 5000 ms`,
    `- highlight latency p95 < 200 ms`,
    `- drag FPS ≥ 30`,
    `- heap growth between scales should be sublinear-or-linear in node count, not quadratic`,
    ``,
    `## Bottleneck Analysis`,
    ``,
    `_To be populated by Task 2 of the plan once the measurements above are reviewed._`,
    ``,
    `## Evidence`,
    ``,
    `- **Video**: \`${recordingPath}\``,
    `- **Screenshots**:`,
    ...screenshotPaths.map((p) => `  - \`${p}\``),
    `- **This report**: \`${join(runDir, "report.md")}\``,
    ``,
  ].join("\n");

  Deno.writeTextFileSync(join(runDir, "report.md"), body);
}

// ---------------------------------------------------------------------------
// Misc helpers
// ---------------------------------------------------------------------------

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function ts(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}_${pad(d.getHours())}-${pad(d.getMinutes())}`;
}

function readPackageVersions(repoRoot: string): { xyflow?: string; dagre?: string } {
  try {
    const raw = Deno.readTextFileSync(join(repoRoot, "web", "package.json"));
    const j = JSON.parse(raw);
    const deps = { ...(j.dependencies ?? {}), ...(j.devDependencies ?? {}) };
    return { xyflow: deps["@xyflow/react"], dagre: deps["dagre"] };
  } catch {
    return {};
  }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

function printUsage(): void {
  console.log(`Usage: deno run --allow-all script/perf-check-browse.ts [options]

Measures \`ravelact browse\` performance at two scales (dogfood + synthetic 300 workflows).
Output is written to \`.wadackel/qa/<timestamp>_browse-perf-300/\`.

Prerequisites:
  - Release binary at ./target/release/ravelact (run \`nix develop -c just build-release\` first).
  - agent-browser state file at \`~/.agent-browser-state/main.json\` (run \`ab-state-refresh\`).

Options:
  --help          Show this help.
`);
}

async function main(): Promise<void> {
  const args = Deno.args;
  if (args.includes("--help") || args.includes("-h")) {
    printUsage();
    return;
  }

  const scriptDir = dirname(fromFileUrl(import.meta.url));
  const repoRoot = resolve(scriptDir, "..");
  const binaryPath = join(repoRoot, "target", "release", "ravelact");

  // Binary existence check.
  try {
    const stat = await Deno.stat(binaryPath);
    if (!stat.isFile) throw new Error("not a file");
  } catch {
    console.error(`ravelact binary not found at ${binaryPath}.`);
    console.error("Run: nix develop -c just build-release first");
    Deno.exit(1);
  }

  const runDir = join(repoRoot, ".wadackel", "qa", `${ts()}_browse-perf-300`);
  Deno.mkdirSync(join(runDir, "screenshots"), { recursive: true });
  console.log(`Run directory: ${runDir}`);

  // Start agent-browser session + recording.
  const recordingPath = join(runDir, "recording.webm");
  await ab(["record", "start", recordingPath], { stateOnFirst: true });

  let dogfood: ScaleMetrics | undefined;
  let at300: ScaleMetrics | undefined;
  let mirrorCheck: { total: number; reusable: number } = { total: 0, reusable: 0 };

  try {
    dogfood = await measureScale({
      label: "dogfood",
      root: repoRoot,
      binaryPath,
      workflows: -1,
      isFirst: false, // recording start already initialised the session
      runDir,
    });

    const synthDir = await Deno.makeTempDir({ prefix: "ravelact-perf-300-" });
    writeSyntheticEstate(synthDir, 300);
    mirrorCheck = await verifyMirrorAgainstRust(binaryPath, synthDir);
    if (mirrorCheck.total !== 300) {
      throw new Error(`mirror check: expected 300 workflows, got ${mirrorCheck.total}`);
    }
    if (mirrorCheck.reusable !== 30) {
      throw new Error(`mirror check: expected 30 reusable, got ${mirrorCheck.reusable}`);
    }

    at300 = await measureScale({
      label: "synthetic-300",
      root: synthDir,
      binaryPath,
      workflows: 300,
      isFirst: false,
      runDir,
    });
  } finally {
    try {
      await ab(["record", "stop"]);
    } catch {
      // already stopped or session died
    }
  }

  if (!dogfood || !at300) {
    console.error("measurement aborted before both scales completed");
    Deno.exit(2);
  }

  writeReport({
    runDir,
    dogfood,
    at300,
    mirrorCheck,
    pkgVersions: readPackageVersions(repoRoot),
    recordingPath,
    screenshotPaths: [
      join(runDir, "screenshots", "qa-perf-dogfood.png"),
      join(runDir, "screenshots", "qa-perf-synthetic-300.png"),
    ],
  });

  console.log(`\nReport: ${join(runDir, "report.md")}`);
}

if (import.meta.main) {
  await main();
}
