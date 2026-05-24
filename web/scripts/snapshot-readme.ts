#!/usr/bin/env node
/**
 * web/scripts/snapshot-readme.ts — regenerate the README browse
 * screenshots committed under docs/images/browse-*.png.
 *
 * Spawns `ravelact browse --no-open --port 0` against the host repository,
 * drives it via Playwright (Chromium, deterministic viewport / DPR / locale,
 * reduced motion), captures four shots that exercise the Trace / Impact tabs,
 * the OverviewPane event-impact highlight, and the search box, then runs
 * `oxipng -o 4 --strip safe` on each emitted PNG.
 *
 * Plan: ~/.claude/plans/20260523T1128-readme-browse-screenshots.md
 *
 * Runtime: Node.js via tsx. Lives under web/ so module resolution finds
 * the existing @playwright/test install.
 *
 * Prerequisites:
 *   - Release binary at `./target/release/ravelact` — run
 *     `nix develop -c just build-release` first.
 *   - Playwright Chromium in the global cache. Install once with
 *     `cd web && nix develop -c pnpm exec playwright install chromium`.
 *   - `oxipng` on PATH (provided by `nix develop -c`).
 *
 * Invocation: `just snapshot-readme [-- --update] [-- --bin <path>]`
 * which expands to `pnpm --dir web exec tsx scripts/snapshot-readme.ts`.
 *
 * Flags:
 *   --update          Overwrite existing PNGs without comparison.
 *   --bin <path>      Override the ravelact binary path.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { readFile, mkdir, readdir, rename, rm, stat, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { chromium, type Page } from "@playwright/test";

// Test/perf surface installed on `(globalThis as GlobalWithRf).__ravelactRf` by Graph.tsx
// (see `web/src/lib/dev-globals.ts`). Referenced from arrow functions
// serialized into page.evaluate / waitForFunction — those run in the
// browser, not in this Node process.
type GraphNodeKind = "workflow" | "local-action" | "external-action";

// Wider than `web/src/lib/dev-globals.ts`'s `RavelactRf` (which keeps
// `data: unknown` and omits `position` because App.tsx + e2e never need
// them). The snapshot script needs both `data.kind` for local-action
// discovery and `position` for layout-settle digests. We can't
// `declare global` to merge this in — `web/e2e/browse.spec.ts:13-15`
// already publishes the narrower shape — so use a cast type instead.
type RavelactRf = {
  getNodes: () => {
    id: string;
    position: { x: number; y: number };
    data: { kind: GraphNodeKind };
  }[];
  fadedIds: () => string[];
  tapNode: (id: string) => string | null;
  tapFirstWorkflow: () => string | null;
  tapFirstWorkflowExcept: (excludeId: string) => string | null;
  backgroundTap: () => void;
};

type GlobalWithRf = { __ravelactRf?: RavelactRf };

const __dirname = dirname(fileURLToPath(import.meta.url));
// __dirname = <repo>/web/scripts → REPO_ROOT = <repo>
const REPO_ROOT = resolve(__dirname, "..", "..");
const DEFAULT_BIN = join(REPO_ROOT, "target", "release", "ravelact");
const IMAGES_DIR = join(REPO_ROOT, "docs", "images");

const VIEWPORT = { width: 1440, height: 900 };
const DEVICE_SCALE_FACTOR = 2;
const LOCALE = "en-US";
const SETTLE_TIMEOUT_MS = 5_000;
const FETCH_TIMEOUT_MS = 10_000;
const BIND_TIMEOUT_MS = 15_000;

// ---------------------------------------------------------------------------
// argv parsing
// ---------------------------------------------------------------------------

interface CliArgs {
  update: boolean;
  bin: string;
}

function parseArgs(argv: string[]): CliArgs {
  let update = false;
  let bin = DEFAULT_BIN;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--update") {
      update = true;
    } else if (a === "--bin") {
      i += 1;
      const v = argv[i];
      if (!v) throw new Error("--bin requires a path argument");
      bin = resolve(v);
    } else if (a === "--help" || a === "-h") {
      console.log("Usage: just snapshot-readme [-- --update] [-- --bin <path>]");
      process.exit(0);
    } else {
      throw new Error(`unknown argument: ${a}`);
    }
  }
  return { update, bin };
}

// ---------------------------------------------------------------------------
// ravelact browse spawn — mirrors script/perf-check-browse.ts:60-119
// ---------------------------------------------------------------------------

interface SpawnedBrowse {
  proc: ChildProcess;
  port: number;
}

async function spawnBrowse(binaryPath: string, root: string): Promise<SpawnedBrowse> {
  const proc = spawn(binaryPath, ["--root", root, "browse", "--no-open", "--port", "0"], {
    stdio: ["ignore", "pipe", "pipe"],
  });

  return await new Promise<SpawnedBrowse>((resolveFn, rejectFn) => {
    let buf = "";
    let settled = false;
    const finish = (err: Error | null, value?: SpawnedBrowse) => {
      if (settled) return;
      settled = true;
      proc.stdout?.removeAllListeners("data");
      proc.removeAllListeners("exit");
      clearTimeout(timer);
      if (err) {
        terminateChild(proc).finally(() => rejectFn(err));
      } else if (value) {
        resolveFn(value);
      }
    };
    const timer = setTimeout(() => {
      finish(new Error("timed out waiting for ravelact bind announcement"));
    }, BIND_TIMEOUT_MS);
    proc.stdout?.setEncoding("utf-8");
    proc.stdout?.on("data", (chunk: string) => {
      buf += chunk;
      let newlineIdx: number;
      while ((newlineIdx = buf.indexOf("\n")) !== -1) {
        const line = buf.slice(0, newlineIdx);
        buf = buf.slice(newlineIdx + 1);
        const port = parseBindPort(line);
        if (port !== null) {
          finish(null, { proc, port });
          return;
        }
      }
    });
    proc.on("exit", (code) => {
      finish(new Error(`ravelact browse exited before announcing bind (code ${code ?? "null"})`));
    });
  });
}

function parseBindPort(line: string): number | null {
  const m = line.match(/http:\/\/127\.0\.0\.1:(\d+)\//);
  if (!m || !m[1]) return null;
  const n = parseInt(m[1], 10);
  return Number.isFinite(n) ? n : null;
}

async function terminateChild(proc: ChildProcess): Promise<void> {
  if (proc.exitCode !== null || proc.signalCode !== null) return;
  await new Promise<void>((r) => {
    proc.once("exit", () => r());
    try {
      proc.kill("SIGTERM");
    } catch {
      r();
    }
    setTimeout(() => r(), 2_000);
  });
}

// ---------------------------------------------------------------------------
// Playwright Chromium presence check
// ---------------------------------------------------------------------------

function chromiumCacheRoot(): string {
  const home = process.env.HOME ?? "";
  if (process.platform === "darwin") {
    return join(home, "Library", "Caches", "ms-playwright");
  }
  return join(home, ".cache", "ms-playwright");
}

async function ensureChromiumInstalled(): Promise<void> {
  const root = chromiumCacheRoot();
  let exists = false;
  try {
    const s = await stat(root);
    exists = s.isDirectory();
  } catch {
    exists = false;
  }
  if (!exists) {
    throw new Error(
      `Playwright Chromium cache not found at ${root}.\n` +
        `Install it once with:\n` +
        `  cd web && nix develop -c pnpm exec playwright install chromium`,
    );
  }
  const entries = await readdir(root, { withFileTypes: true });
  const hasChromium = entries.some((e) => e.isDirectory() && e.name.startsWith("chromium-"));
  if (!hasChromium) {
    throw new Error(
      `Playwright cache at ${root} is missing a chromium-* directory.\n` +
        `Install it with:\n` +
        `  cd web && nix develop -c pnpm exec playwright install chromium`,
    );
  }
}

// ---------------------------------------------------------------------------
// Graph settle detection
// ---------------------------------------------------------------------------

async function waitForGraphMounted(page: Page): Promise<void> {
  await page.waitForFunction(
    () => {
      const rf = (globalThis as GlobalWithRf).__ravelactRf;
      return Boolean(rf && rf.getNodes().length > 0);
    },
    undefined,
    { timeout: SETTLE_TIMEOUT_MS },
  );
}

async function nodeDigest(page: Page): Promise<string> {
  return await page.evaluate(() => {
    const rf = (globalThis as GlobalWithRf).__ravelactRf;
    if (!rf) return "";
    return rf
      .getNodes()
      .map((n) => `${n.id}:${n.position.x},${n.position.y}`)
      .join("|");
  });
}

async function nextFrame(page: Page): Promise<void> {
  await page.evaluate(() => new Promise<void>((r) => requestAnimationFrame(() => r())));
}

async function waitNoAnimations(page: Page): Promise<void> {
  await page.waitForFunction(
    () => document.getAnimations().every((a) => a.playState !== "running"),
    undefined,
    { timeout: SETTLE_TIMEOUT_MS },
  );
}

async function settleGraph(page: Page): Promise<void> {
  await waitForGraphMounted(page);
  let prev: string | null = null;
  const start = Date.now();
  while (Date.now() - start < SETTLE_TIMEOUT_MS) {
    const d = await nodeDigest(page);
    if (d !== "" && d === prev) break;
    prev = d;
    await nextFrame(page);
  }
  await waitNoAnimations(page);
}

// ---------------------------------------------------------------------------
// Captures
// ---------------------------------------------------------------------------

interface CaptureContext {
  page: Page;
  baseUrl: string;
}

// Playwright's `page.screenshot({ path })` runs `getMimeTypeForPath` and
// rejects anything that does not end in `.png` / `.jpeg`. Our tmp paths
// use a `.new` suffix so we cannot pass them as `path`; capture into a
// Buffer instead and write the file ourselves.
async function writeShot(page: Page, outPath: string): Promise<void> {
  const buf = await page.screenshot({ type: "png" });
  await writeFile(outPath, buf);
}

async function tapFirstWorkflow(page: Page): Promise<string> {
  const id = await page.evaluate(() => {
    const rf = (globalThis as GlobalWithRf).__ravelactRf;
    return rf?.tapFirstWorkflow() ?? null;
  });
  if (!id) throw new Error("no workflow node available to tap");
  return id;
}

async function tapFirstLocalAction(page: Page): Promise<string> {
  const id = await page.evaluate(() => {
    const rf = (globalThis as GlobalWithRf).__ravelactRf;
    if (!rf) return null;
    const node = rf.getNodes().find((n) => n.data.kind === "local-action");
    if (!node) return null;
    return rf.tapNode(node.id);
  });
  if (!id) throw new Error("no local-action node available to tap");
  return id;
}

async function backgroundTap(page: Page): Promise<void> {
  await page.evaluate(() => {
    const rf = (globalThis as GlobalWithRf).__ravelactRf;
    rf?.backgroundTap();
  });
}

async function waitForFadedNonEmpty(page: Page): Promise<void> {
  await page.waitForFunction(
    () => {
      const rf = (globalThis as GlobalWithRf).__ravelactRf;
      return Boolean(rf && rf.fadedIds().length > 0);
    },
    undefined,
    { timeout: FETCH_TIMEOUT_MS },
  );
}

const BROWSE_SERVICE = "ravelact.browse.v1.BrowseService";
type BrowseRpcMethod = "Trace" | "GetEventImpact" | "GetImpact" | "Search";

// ConnectRPC unary routes are `/<service>/<method>`; Connect-GET appends `?…`.
// Anchor on `?` or end-of-string so a future sibling method that shares a
// prefix (hypothetical `TraceDeep`) cannot satisfy the matcher.
function waitForRpc(page: Page, method: BrowseRpcMethod) {
  const re = new RegExp(`/${BROWSE_SERVICE.replace(/\./g, "\\.")}/${method}(?:\\?|$)`);
  return page.waitForResponse((r) => re.test(r.url()) && r.ok(), {
    timeout: FETCH_TIMEOUT_MS,
  });
}

async function captureHero(ctx: CaptureContext, outPath: string): Promise<string> {
  await ctx.page.goto(ctx.baseUrl);
  await settleGraph(ctx.page);
  const id = await tapFirstWorkflow(ctx.page);
  await ctx.page.waitForSelector('[role="tab"][data-tab="trace"]');
  const tracePromise = waitForRpc(ctx.page, "Trace");
  await ctx.page.click('[role="tab"][data-tab="trace"]');
  await tracePromise;
  await ctx.page.waitForSelector('[role="tab"][data-tab="trace"][aria-selected="true"]');
  await settleGraph(ctx.page);
  await writeShot(ctx.page, outPath);
  return id;
}

// Pin the chosen event so the screenshot stays in sync with the README's
// alt text ("push trigger event selected"). Picking "the first enabled
// button" would silently drift if OverviewPane's render order changes.
const OVERVIEW_EVENT = "push";

async function captureOverview(ctx: CaptureContext, outPath: string): Promise<void> {
  await backgroundTap(ctx.page);
  await ctx.page.waitForSelector('aside[aria-label="Graph overview"]');
  const eventImpactPromise = waitForRpc(ctx.page, "GetEventImpact");
  await ctx.page
    .locator('aside[aria-label="Graph overview"] button', { hasText: OVERVIEW_EVENT })
    .first()
    .click();
  await eventImpactPromise;
  await waitForFadedNonEmpty(ctx.page);
  await settleGraph(ctx.page);
  await writeShot(ctx.page, outPath);
}

async function captureNodeDetail(ctx: CaptureContext, outPath: string): Promise<void> {
  // Local actions in this repo (e.g. setup-nix-devshell) are reused
  // across multiple workflows, so the Impact tab renders a populated
  // IMPACTED WORKFLOWS list — a more compelling narrative than tapping
  // a top-level workflow whose Impact is empty.
  await tapFirstLocalAction(ctx.page);
  await ctx.page.waitForSelector('[role="tab"][data-tab="impact"]');
  const impactPromise = waitForRpc(ctx.page, "GetImpact");
  await ctx.page.click('[role="tab"][data-tab="impact"]');
  await impactPromise;
  await ctx.page.waitForSelector('[role="tab"][data-tab="impact"][aria-selected="true"]');
  await settleGraph(ctx.page);
  await writeShot(ctx.page, outPath);
}

async function captureSearch(ctx: CaptureContext, outPath: string): Promise<void> {
  await backgroundTap(ctx.page);
  await ctx.page.waitForSelector('input[aria-label="Search nodes, files, and triggers"]');
  // Register the response listener BEFORE the fill so the
  // App.tsx:41 debounce (120ms) does not race ahead of us.
  const searchPromise = waitForRpc(ctx.page, "Search");
  await ctx.page.fill('input[aria-label="Search nodes, files, and triggers"]', "ci");
  await searchPromise;
  await waitForFadedNonEmpty(ctx.page);
  await settleGraph(ctx.page);
  await writeShot(ctx.page, outPath);
}

// ---------------------------------------------------------------------------
// oxipng post-processing
// ---------------------------------------------------------------------------

async function runOxipng(path: string): Promise<void> {
  await new Promise<void>((resolveFn, rejectFn) => {
    const child = spawn("oxipng", ["-o", "4", "--strip", "safe", path], {
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stderr = "";
    child.stderr?.setEncoding("utf-8");
    child.stderr?.on("data", (chunk: string) => (stderr += chunk));
    child.on("error", rejectFn);
    child.on("exit", (code) => {
      if (code === 0) resolveFn();
      else rejectFn(new Error(`oxipng ${path} failed (code ${code}): ${stderr.trim()}`));
    });
  });
}

// ---------------------------------------------------------------------------
// Diff / commit logic
// ---------------------------------------------------------------------------

async function readFileIfExists(path: string): Promise<Buffer | null> {
  try {
    return await readFile(path);
  } catch (e) {
    if ((e as NodeJS.ErrnoException).code === "ENOENT") return null;
    throw e;
  }
}

interface ShotResult {
  name: string;
  finalPath: string;
  tmpPath: string;
}

async function finalize(
  shots: ShotResult[],
  update: boolean,
): Promise<{ changed: string[]; added: string[]; same: string[] }> {
  const changed: string[] = [];
  const added: string[] = [];
  const same: string[] = [];
  for (const s of shots) {
    await runOxipng(s.tmpPath);
    const fresh = await readFile(s.tmpPath);
    const existing = await readFileIfExists(s.finalPath);
    if (existing && existing.equals(fresh)) {
      same.push(s.name);
      await rm(s.tmpPath);
      continue;
    }
    const bucket = existing ? changed : added;
    if (!update) {
      // Drift (or first-run absence) without `--update`: leave the `.new`
      // file in place for inspection. Do NOT commit to `finalPath`.
      bucket.push(s.name);
      continue;
    }
    await rename(s.tmpPath, s.finalPath);
    bucket.push(s.name);
  }
  return { changed, added, same };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  const args = parseArgs(process.argv.slice(2));

  try {
    const s = await stat(args.bin);
    if (!s.isFile()) throw new Error(`not a file: ${args.bin}`);
  } catch {
    throw new Error(
      `ravelact binary not found at ${args.bin}.\n` +
        `Run \`nix develop -c just build-release\` first, or pass --bin <path>.`,
    );
  }
  await ensureChromiumInstalled();
  await mkdir(IMAGES_DIR, { recursive: true });

  const spawned = await spawnBrowse(args.bin, REPO_ROOT);
  const baseUrl = `http://127.0.0.1:${spawned.port}/`;

  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({
      viewport: VIEWPORT,
      deviceScaleFactor: DEVICE_SCALE_FACTOR,
      locale: LOCALE,
      reducedMotion: "reduce",
    });
    const page = await context.newPage();
    const ctx: CaptureContext = { page, baseUrl };

    const tmpSuffix = ".new";
    const shot = (name: string): ShotResult => ({
      name,
      finalPath: join(IMAGES_DIR, `${name}.png`),
      tmpPath: join(IMAGES_DIR, `${name}.png${tmpSuffix}`),
    });
    const heroShot = shot("browse-hero");
    const overviewShot = shot("browse-overview");
    const nodeDetailShot = shot("browse-node-detail");
    const searchShot = shot("browse-search");

    // Order matters: search and node-detail must run before overview so
    // the event-impact filter set by captureOverview does not leak into
    // their backgrounds.
    await captureHero(ctx, heroShot.tmpPath);
    await captureNodeDetail(ctx, nodeDetailShot.tmpPath);
    await captureSearch(ctx, searchShot.tmpPath);
    await captureOverview(ctx, overviewShot.tmpPath);

    const { changed, added, same } = await finalize(
      [heroShot, overviewShot, nodeDetailShot, searchShot],
      args.update,
    );

    for (const name of same) console.log(`unchanged: ${name}.png`);
    for (const name of changed) {
      if (args.update) console.log(`updated:   ${name}.png`);
      else console.log(`changed:   ${name}.png (kept .new — re-run with --update to commit)`);
    }
    for (const name of added) {
      if (args.update) console.log(`added:     ${name}.png`);
      else console.log(`new:       ${name}.png (kept .new — re-run with --update to commit)`);
    }

    if (!args.update && changed.length + added.length > 0) {
      const parts: string[] = [];
      if (changed.length) parts.push(`${changed.length} differing`);
      if (added.length) parts.push(`${added.length} new`);
      throw new Error(
        `${parts.join(" + ")} screenshot(s) need committing. Inspect the .new files ` +
          `under ${IMAGES_DIR}, then re-run with --update if the diff is intentional.`,
      );
    }
  } finally {
    await browser.close();
    await terminateChild(spawned.proc);
  }
}

main().catch((e) => {
  console.error(e instanceof Error ? e.message : String(e));
  process.exit(1);
});
