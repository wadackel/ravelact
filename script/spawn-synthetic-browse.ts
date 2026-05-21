#!/usr/bin/env -S deno run --allow-all
/**
 * script/spawn-synthetic-browse.ts — interactive driver for the
 * synthetic estate the perf harness and integration tests use.
 *
 * Writes N synthetic workflows under a tempdir, then runs
 * `ravelact browse --root <tempdir>` in the foreground so you can
 * actually navigate the high-scale UI yourself. Ctrl+C tears
 * everything down (server + tempdir).
 *
 * Usage:
 *   nix develop -c just dev-synthetic              # 300 workflows (default)
 *   nix develop -c just dev-synthetic 1000         # custom scale
 *   deno run --allow-all script/spawn-synthetic-browse.ts 300 --port 7878 --no-open
 *
 * Prerequisites:
 *   - Release binary at ./target/release/ravelact
 *     (run `nix develop -c just build-release` first)
 *
 * The synthetic shape — 30 reusable + N-30 callers, each step uses
 * actions/checkout@v4 — must stay in lockstep with
 * `tests/e2e_browse.rs::write_synthetic_estate`. Update both when the
 * fixture shape changes.
 */

import { dirname, fromFileUrl, join, resolve } from "jsr:@std/path@1";

function writeSyntheticEstate(dir: string, workflows: number): void {
  const wfDir = join(dir, ".github", "workflows");
  Deno.mkdirSync(wfDir, { recursive: true });
  const reusableCount = Math.min(workflows, 30);
  for (let i = 0; i < workflows; i++) {
    const idx = String(i).padStart(3, "0");
    const path = join(wfDir, `wf-${idx}.yaml`);
    const content = i < reusableCount
      ? `name: Reusable ${i}\non:\n  workflow_call:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: echo wf-${i}\n`
      : `name: Caller ${i}\non:\n  push:\njobs:\n  build:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v4\n      - run: echo wf-${i}\n  call:\n    uses: ./.github/workflows/wf-${
        String(i % reusableCount).padStart(3, "0")
      }.yaml\n`;
    Deno.writeTextFileSync(path, content);
  }
}

function parseArgs(argv: string[]): {
  workflows: number;
  port: number;
  open: boolean;
} {
  let workflows = 300;
  let port = 7878;
  let open = true;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--no-open") open = false;
    else if (a === "--port") port = parseInt(argv[++i] ?? "", 10);
    else if (/^\d+$/.test(a)) workflows = parseInt(a, 10);
    else {
      console.error(`unknown argument: ${a}`);
      Deno.exit(2);
    }
  }
  if (!Number.isFinite(workflows) || workflows < 1) {
    console.error(`workflows must be a positive integer; got ${workflows}`);
    Deno.exit(2);
  }
  if (!Number.isFinite(port) || port < 1 || port > 65535) {
    console.error(`port must be 1..65535; got ${port}`);
    Deno.exit(2);
  }
  return { workflows, port, open };
}

async function main() {
  const { workflows, port, open } = parseArgs(Deno.args);
  const repoRoot = resolve(
    dirname(fromFileUrl(import.meta.url)),
    "..",
  );
  const binary = join(repoRoot, "target", "release", "ravelact");
  try {
    Deno.statSync(binary);
  } catch {
    console.error(
      `ravelact release binary not found at ${binary}\n` +
        `Run: nix develop -c just build-release`,
    );
    Deno.exit(1);
  }

  const synthDir = Deno.makeTempDirSync({ prefix: "ravelact-synthetic-" });
  console.log(`Generating ${workflows} synthetic workflows under ${synthDir}`);
  writeSyntheticEstate(synthDir, workflows);

  const args = ["--root", synthDir, "browse", "--port", String(port)];
  if (!open) args.push("--no-open");

  console.log(`Launching: ${binary} ${args.join(" ")}`);
  console.log("Ctrl+C to stop (tempdir is cleaned up automatically).\n");

  const child = new Deno.Command(binary, {
    args,
    stdout: "inherit",
    stderr: "inherit",
  }).spawn();

  const cleanup = () => {
    try {
      Deno.removeSync(synthDir, { recursive: true });
    } catch {
      // best-effort: a SIGKILL'd parent may leak the tempdir; the OS
      // sweeps /tmp eventually.
    }
  };
  // Forward Ctrl+C / SIGTERM to the child so it exits cleanly, then
  // let `await child.status` resolve so the `finally` block can run
  // the tempdir cleanup. SIGKILL of *this* process skips the finally
  // entirely — accepted limitation.
  const forward = (sig: Deno.Signal) => {
    try {
      child.kill(sig);
    } catch {
      // child already gone
    }
  };
  Deno.addSignalListener("SIGINT", () => forward("SIGINT"));
  Deno.addSignalListener("SIGTERM", () => forward("SIGTERM"));

  let code = 0;
  try {
    const status = await child.status;
    code = status.success ? 0 : status.code;
  } finally {
    cleanup();
  }
  Deno.exit(code);
}

if (import.meta.main) {
  await main();
}
