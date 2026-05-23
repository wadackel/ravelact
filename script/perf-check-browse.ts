#!/usr/bin/env -S deno run --allow-all
/**
 * script/perf-check-browse.ts — `ravelact browse` performance harness.
 *
 * Measures the React 19 + ReactFlow (@xyflow/react) SPA across the
 * scales selected via `--scales` (default `300,5000`), plus dogfood:
 *   - dogfood     : the host repo (~15 nodes, ~37 edges) — always measured.
 *   - synthetic-N : N generated workflows (30 reusable + N-30 callers).
 *
 * Output:
 *   .wadackel/qa/<YYYY-MM-DD_HH-MM>_<label>/
 *     ├── report.md            (metric tables + methodology + screenshots list)
 *     ├── recording.webm       (one continuous recording for the whole session)
 *     └── screenshots/         (per-scale before/after captures)
 *   `--label` controls the dir-name suffix so baseline vs PR runs do not collide.
 *
 * Prerequisites:
 *   - Release binary at ./target/release/ravelact (run `nix develop -c just build-release`)
 *   - agent-browser state file at ~/.agent-browser-state/main.json (run `ab-state-refresh`)
 *   - Deno on PATH (not yet wired into flake.nix dev shell)
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
  // EOF-as-fatal (child exited before binding). When EOF or timeout
  // hits, drain stderr to include the child's diagnostic output in the
  // surfaced error — otherwise the harness reports a useless "exited"
  // message with no clue why.
  const decoder = new TextDecoder();
  const reader = proc.stdout.getReader();
  const deadline = Date.now() + 15_000;
  let buf = "";
  try {
    while (true) {
      if (Date.now() >= deadline) {
        const stderrTail = await readChildStderr(proc);
        await terminateChild(proc);
        throw new Error(
          `timed out waiting for ravelact bind announcement${stderrTail ? `; stderr: ${stderrTail}` : ""}`,
        );
      }
      const { value, done } = await reader.read();
      if (done) {
        const stderrTail = await readChildStderr(proc);
        await terminateChild(proc);
        throw new Error(
          `ravelact browse exited before announcing bind (EOF on stdout)${stderrTail ? `; stderr: ${stderrTail}` : ""}`,
        );
      }
      buf += decoder.decode(value, { stream: true });
      const newlineIdx = buf.indexOf("\n");
      if (newlineIdx < 0) continue;
      const line = buf.slice(0, newlineIdx);
      buf = buf.slice(newlineIdx + 1);
      const port = parseBindPort(line);
      if (port !== null) {
        // After the bind announcement the child keeps writing log lines
        // to stdout/stderr ("press Ctrl+C to stop", etc.). Without
        // draining, the pipe buffers eventually fill and block the
        // child. Spawn fire-and-forget drains that exit when the pipes
        // close (i.e. when the child is terminated).
        reader.releaseLock();
        drainPipe(proc.stdout);
        drainPipe(proc.stderr);
        return { proc, port };
      }
    }
  } catch (e) {
    // Re-throw after ensuring the reader is released; the outer caller
    // owns no other reference to the process at this point.
    try {
      reader.releaseLock();
    } catch {
      // already released by the bind-announcement path
    }
    throw e;
  }
}

function drainPipe(stream: ReadableStream<Uint8Array>): void {
  // Read-and-discard loop. Promise is intentionally not awaited — it
  // resolves only when the child exits and the pipe closes.
  void (async () => {
    try {
      const r = stream.getReader();
      while (true) {
        const { done } = await r.read();
        if (done) break;
      }
    } catch {
      // pipe closed / child gone — fine.
    }
  })();
}

async function readChildStderr(proc: Deno.ChildProcess): Promise<string> {
  // Best-effort drain of whatever stderr has buffered so far. Single
  // read with a 200 ms cap — the harness only needs a hint, not a
  // full log. We deliberately do not loop: on the timeout branch the
  // pending `r.read()` would orphan, and the next loop iteration would
  // throw `reader is locked / already reading`. Cancelling the reader
  // in finally resolves the orphan with `done:true` and unlocks.
  const r = proc.stderr.getReader();
  const decoder = new TextDecoder();
  try {
    const race = await Promise.race<{ value?: Uint8Array; done: boolean }>([
      r.read(),
      new Promise((resolve) => setTimeout(() => resolve({ done: true }), 200)),
    ]);
    if (race.done || !race.value) return "";
    return decoder.decode(race.value).trim().slice(-512);
  } catch {
    return "";
  } finally {
    try {
      await r.cancel();
    } catch {
      // ignore — stream already closed / cancelled
    }
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
  // Give the child 5 s to exit cleanly, then escalate to SIGKILL.
  // Without this, a child that ignores SIGTERM (hung / debugger
  // attached / etc.) would block this script forever on `proc.status`.
  const killTimer = setTimeout(() => {
    try {
      proc.kill("SIGKILL");
    } catch {
      // already gone
    }
  }, 5_000);
  try {
    await proc.status;
  } catch {
    // ignore
  } finally {
    clearTimeout(killTimer);
  }
}

// ---------------------------------------------------------------------------
// agent-browser thin wrapper
// ---------------------------------------------------------------------------

const AB_SESSION = `claude-${Deno.pid}`;
function requireHome(): string {
  const home = Deno.env.get("HOME");
  if (!home) {
    throw new Error(
      "HOME is not set; agent-browser state file path cannot be resolved. Set HOME or run from a shell that exports it.",
    );
  }
  return home;
}
const AB_STATE = `${requireHome()}/.agent-browser-state/main.json`;

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

async function waitForGraphReady(deadlineMs = 30_000): Promise<NavigationResult> {
  // Poll for __ravelactRf + node count > 0; record timeOrigin and
  // the first moment all readiness conditions hold. After the dagre
  // worker offload `__ravelactRf` is installed only post-resolve, so
  // the existence of `getNodes().length > 0` is the canonical signal.
  // Callers measuring 5k-scale estates need a larger deadlineMs because
  // Worker postMessage roundtrip + dagre on 5000 nodes can exceed the
  // 30 s dogfood baseline.
  const deadline = Date.now() + deadlineMs;
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
  readyTimeoutMs?: number;
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
    const nav = await waitForGraphReady(opts.readyTimeoutMs);
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
  if (!dump.success) {
    const stderr = new TextDecoder().decode(dump.stderr).trim();
    throw new Error(`ravelact dump failed${stderr ? `: ${stderr}` : ""}`);
  }
  const json: unknown = JSON.parse(new TextDecoder().decode(dump.stdout));
  // Guard the shape we expect. The Rust dump output is a stable contract,
  // but typing `JSON.parse` as `unknown` and narrowing forces us to
  // surface a clear error if the contract drifts instead of silently
  // returning total=0.
  if (typeof json !== "object" || json === null || !Array.isArray((json as { workflows?: unknown }).workflows)) {
    throw new Error(`ravelact dump output: expected object with .workflows array, got ${typeof json}`);
  }
  const workflows = (json as { workflows: unknown[] }).workflows;
  const total = workflows.length;
  let reusable = 0;
  for (const w of workflows) {
    if (typeof w !== "object" || w === null) continue;
    const triggers = (w as { triggers?: unknown }).triggers;
    if (!Array.isArray(triggers)) continue;
    if (
      triggers.some(
        (t) =>
          typeof t === "object" &&
          t !== null &&
          typeof (t as { event?: { kind?: unknown } }).event === "object" &&
          (t as { event: { kind?: unknown } }).event !== null &&
          (t as { event: { kind?: unknown } }).event.kind === "workflow_call",
      )
    ) {
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
  scales: ScaleMetrics[];
  mirrorChecks: Map<string, { total: number; reusable: number }>;
  pkgVersions: { xyflow?: string; dagre?: string };
  recordingPath: string;
  screenshotPaths: string[];
  scenarioLabel: string;
}): void {
  const { runDir, scales, mirrorChecks, pkgVersions, recordingPath, screenshotPaths, scenarioLabel } = args;
  const fmtFps = (f: number) => f.toFixed(1);
  const fmtMs = (n: number) => `${n.toFixed(1)} ms`;
  const fmtSettle = (s: number | "capped") => s === "capped" ? "> 5000 ms (capped)" : `${(s as number).toFixed(1)} ms`;

  const headerRow = `| Metric | ${scales.map((s) => s.label).join(" | ")} |`;
  const sepRow = `|---|${scales.map(() => "---").join("|")}|`;
  const valueRow = (label: string, render: (s: ScaleMetrics) => string) =>
    `| ${label} | ${scales.map(render).join(" | ")} |`;

  const mirrorLines: string[] = [];
  for (const s of scales) {
    if (s.workflows < 0) continue; // dogfood has no synthetic mirror
    const m = mirrorChecks.get(s.label);
    if (!m) continue;
    const expectedReusable = Math.min(s.workflows, 30);
    mirrorLines.push(
      `  - **${s.label}**: total=${m.total} (expected ${s.workflows}), reusable=${m.reusable} (expected ${expectedReusable}).`,
    );
  }

  const body = [
    `# Browse perf report — ${scenarioLabel}`,
    ``,
    `Generated: ${new Date().toISOString()}`,
    ``,
    `## Methodology`,
    ``,
    `- Harness: \`script/perf-check-browse.ts\` (Deno).`,
    `- Scales measured back-to-back in the same Chrome instance via agent-browser:`,
    ...scales.map((s) =>
      s.workflows < 0
        ? `  - **${s.label}**: host repo (~${s.node_count} nodes / ${s.edge_count} edges).`
        : `  - **${s.label}**: TempDir with ${s.workflows} generated workflows (${Math.min(s.workflows, 30)} reusable + ${s.workflows - Math.min(s.workflows, 30)} caller).`,
    ),
    `- Scenario label: \`${scenarioLabel}\` — embed this into the parent QA report so a baseline vs PR-branch comparison aligns scales 1:1.`,
    `- Synthetic estate generation is **TS file I/O before browser navigation** — its time is NOT included in any "initial load" number reported here.`,
    `- Versions: see \`web/package.json\`. Detected at run-time: @xyflow/react=${pkgVersions.xyflow ?? "unknown"}, dagre=${pkgVersions.dagre ?? "unknown"}.`,
    `- TS↔Rust mirror cross-check:`,
    ...(mirrorLines.length ? mirrorLines : [`  - (no synthetic scales — dogfood only)`]),
    `- "Coarse heap snapshot" — \`performance.memory.usedJSHeapSize\` is bucketed to ~100 KB; small leaks below that resolution are invisible.`,
    `- "Drag FPS" is sampled during scripted \`rf.panBy\` for ≥ 3 s. ReactFlow pans by mutating the viewport's CSS transform, so per-frame cost is style recompute + composite (no canvas redraw).`,
    `- "Settle time" is the duration after a single pan until the viewport-element mutation stream stays quiet ≥ 100 ms; ceiling 5 s. A \`MutationObserver\` on \`.react-flow__viewport\` style/transform attributes is the source.`,
    `- "Highlight latency" is \`performance.measure('highlight', 'perf:tap-enter', 'perf:faded-applied')\` across up to 20 distinct workflow nodes; p50/p95 reported.`,
    ``,
    `## Results`,
    ``,
    headerRow,
    sepRow,
    valueRow("nodes (rf.getNodes().length)", (s) => String(s.node_count)),
    valueRow("edges (rf.getEdges().length)", (s) => String(s.edge_count)),
    valueRow("/api/graph size", (s) => fmtBytes(s.api_graph_bytes)),
    valueRow("initial load (timeOrigin → first ready)", (s) => fmtMs(s.initial_load_ms)),
    valueRow("drag FPS (3 s sample)", (s) => fmtFps(s.drag_fps)),
    valueRow("settle after pan", (s) => fmtSettle(s.settle_ms)),
    valueRow(
      "highlight latency p50 (n samples)",
      (s) => `${fmtMs(s.highlight_latency_ms.p50)} (n=${s.highlight_latency_ms.samples})`,
    ),
    valueRow("highlight latency p95", (s) => fmtMs(s.highlight_latency_ms.p95)),
    valueRow("heap initial (coarse)", (s) => fmtBytes(s.heap_initial_bytes)),
    valueRow("heap after 20 taps (coarse)", (s) => fmtBytes(s.heap_after_interactions_bytes)),
    valueRow("viewport mutation events observed", (s) => String(s.viewport_mutation_event_count)),
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
    `_To be populated by the parent QA report once the measurements above are paired with the baseline scenario._`,
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

Measures \`ravelact browse\` performance across multiple scales (dogfood + synthetic).
Output is written to \`.wadackel/qa/<timestamp>_<run-dir-suffix>/\`.

Prerequisites:
  - Release binary at ./target/release/ravelact (run \`nix develop -c just build-release\` first).
  - agent-browser state file at \`~/.agent-browser-state/main.json\` (run \`ab-state-refresh\`).

Options:
  --scales <csv>      Comma-separated synthetic scales to measure in addition to
                      dogfood. Defaults to \`300,5000\`. Use \`--scales=\` (empty)
                      to skip synthetic scales entirely (dogfood-only smoke).
  --label <label>     Scenario label embedded into the report header and run-dir
                      suffix. Defaults to \`browse-perf-worker-vp\`. Use this so a
                      baseline vs PR-branch invocation produces distinct dirs.
  --help              Show this help.
`);
}

function parseScales(arg: string | undefined): number[] {
  if (arg === undefined) return [300, 5000];
  if (arg.trim() === "") return [];
  return arg.split(",").map((s) => {
    const n = parseInt(s.trim(), 10);
    if (!Number.isFinite(n) || n <= 0) {
      throw new Error(`invalid --scales value: ${s}`);
    }
    return n;
  });
}

function parseArgs(argv: string[]): { scales: number[]; label: string } {
  let scalesArg: string | undefined;
  let label = "browse-perf-worker-vp";
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i] ?? "";
    if (a === "--scales") scalesArg = argv[++i];
    else if (a.startsWith("--scales=")) scalesArg = a.slice("--scales=".length);
    else if (a === "--label") label = argv[++i] ?? label;
    else if (a.startsWith("--label=")) label = a.slice("--label=".length);
  }
  return { scales: parseScales(scalesArg), label };
}

async function main(): Promise<void> {
  const args = Deno.args;
  if (args.includes("--help") || args.includes("-h")) {
    printUsage();
    return;
  }
  const { scales: syntheticScales, label } = parseArgs(args);

  const scriptDir = dirname(fromFileUrl(import.meta.url));
  const repoRoot = resolve(scriptDir, "..");
  const binaryPath = join(repoRoot, "target", "release", "ravelact");

  // Binary existence check. Differentiate "missing file" (most common
  // failure mode — operator forgot to `just build-release`) from other
  // I/O errors (permission, ENOTDIR, etc.) so the diagnostic message
  // actually helps when something unexpected breaks.
  try {
    const stat = await Deno.stat(binaryPath);
    if (!stat.isFile) {
      console.error(`Expected file at ${binaryPath}, got non-file entry.`);
      Deno.exit(1);
    }
  } catch (e) {
    if (e instanceof Deno.errors.NotFound) {
      console.error(`ravelact binary not found at ${binaryPath}.`);
      console.error("Run: nix develop -c just build-release first");
      Deno.exit(1);
    }
    throw e;
  }

  const runDir = join(repoRoot, ".wadackel", "qa", `${ts()}_${label}`);
  Deno.mkdirSync(join(runDir, "screenshots"), { recursive: true });
  console.log(`Run directory: ${runDir}`);
  console.log(`Synthetic scales: ${syntheticScales.length ? syntheticScales.join(", ") : "(none — dogfood only)"}`);

  // Start agent-browser session + recording.
  const recordingPath = join(runDir, "recording.webm");
  await ab(["record", "start", recordingPath], { stateOnFirst: true });

  const measurements: ScaleMetrics[] = [];
  const mirrorChecks = new Map<string, { total: number; reusable: number }>();
  // Track temp dirs so we can clean them up after the run completes.
  const tempDirs: string[] = [];

  // Best-effort cleanup on Ctrl-C / SIGTERM. Without these, an
  // interrupted harness leaks the tempdir + ongoing agent-browser
  // recording (and the recording process can hold the daemon open).
  // Listeners are removed after the main try/finally completes so a
  // normal-completion exit does not run cleanup twice.
  let interrupted = false;
  const onSignal = () => {
    if (interrupted) return;
    interrupted = true;
    console.error("\nperf-check-browse: interrupted, cleaning up …");
    void ab(["record", "stop"]).catch(() => {});
    for (const d of tempDirs) {
      try {
        Deno.removeSync(d, { recursive: true });
      } catch {
        // best-effort
      }
    }
    Deno.exit(130);
  };
  Deno.addSignalListener("SIGINT", onSignal);
  Deno.addSignalListener("SIGTERM", onSignal);

  try {
    measurements.push(
      await measureScale({
        label: "dogfood",
        root: repoRoot,
        binaryPath,
        workflows: -1,
        isFirst: false, // recording start already initialised the session
        runDir,
        readyTimeoutMs: 30_000,
      }),
    );

    for (const wf of syntheticScales) {
      const scaleLabel = `synthetic-${wf}`;
      const synthDir = await Deno.makeTempDir({ prefix: `ravelact-perf-${wf}-` });
      tempDirs.push(synthDir);
      writeSyntheticEstate(synthDir, wf);
      const mc = await verifyMirrorAgainstRust(binaryPath, synthDir);
      if (mc.total !== wf) {
        throw new Error(`mirror check (${scaleLabel}): expected ${wf} workflows, got ${mc.total}`);
      }
      const expectedReusable = Math.min(wf, 30);
      if (mc.reusable !== expectedReusable) {
        throw new Error(
          `mirror check (${scaleLabel}): expected ${expectedReusable} reusable, got ${mc.reusable}`,
        );
      }
      mirrorChecks.set(scaleLabel, mc);

      // 5k-scale dagre on the synthetic star-shaped topology (30 reusable
      // + N-30 callers each calling one reusable) is unusually expensive —
      // baseline measurement showed ~91s wall-clock at 5k. Give a 3-minute
      // floor for any scale >= 1000 so the harness reliably captures the
      // number rather than timing out before paint.
      const readyTimeoutMs = wf >= 1000 ? 180_000 : 30_000;
      measurements.push(
        await measureScale({
          label: scaleLabel,
          root: synthDir,
          binaryPath,
          workflows: wf,
          isFirst: false,
          runDir,
          readyTimeoutMs,
        }),
      );
    }
  } finally {
    try {
      await ab(["record", "stop"]);
    } catch {
      // already stopped or session died
    }
    for (const d of tempDirs) {
      try {
        await Deno.remove(d, { recursive: true });
      } catch {
        // best-effort
      }
    }
    // Normal completion path — drop the signal handlers so they cannot
    // fire during whatever comes after main() returns.
    try {
      Deno.removeSignalListener("SIGINT", onSignal);
      Deno.removeSignalListener("SIGTERM", onSignal);
    } catch {
      // ignore
    }
  }

  const expectedScales = 1 + syntheticScales.length;
  if (measurements.length !== expectedScales) {
    console.error(
      `measurement aborted: completed ${measurements.length}/${expectedScales} scales`,
    );
    Deno.exit(2);
  }

  writeReport({
    runDir,
    scales: measurements,
    mirrorChecks,
    pkgVersions: readPackageVersions(repoRoot),
    recordingPath,
    screenshotPaths: measurements.map((s) => join(runDir, "screenshots", `qa-perf-${s.label}.png`)),
    scenarioLabel: label,
  });

  console.log(`\nReport: ${join(runDir, "report.md")}`);
}

if (import.meta.main) {
  await main();
}
