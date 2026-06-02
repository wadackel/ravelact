import { defineConfig } from "@playwright/test";

// Two-server harness:
//
//   1. Vite dev server on :5173 with `/ravelact.browse.v1.BrowseService/*`
//      proxied to ravelact at :7879 (set via the `RAVELACT_PORT` env var
//      read in `vite.config.ts`).
//   2. ravelact `browse` on :7879 — separate from the dev workflow's :7878
//      so a developer running `pnpm dev` in another shell does not collide
//      when `pnpm e2e` is invoked. `reuseExistingServer: true` lets a
//      pre-warmed instance be reused locally.
//
// In CI the binary must already exist at `../target/release/ravelact`
// (produced by `just frontend && cargo build --release`, see justfile).
//
// The ravelact health-check URL is the SPA index served at `/`. The
// ConnectRPC endpoints under `/ravelact.browse.v1.BrowseService/*`
// require POST and would 405 a GET probe, so we point Playwright at
// the static index instead.

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://localhost:5173",
    trace: "on-first-retry",
  },
  webServer: [
    {
      command: "pnpm dev",
      url: "http://localhost:5173",
      reuseExistingServer: !process.env.CI,
      timeout: 60_000,
      env: {
        RAVELACT_PORT: "7879",
      },
    },
    {
      command: "../target/release/ravelact --root .. browse --no-open --port 7879",
      url: "http://localhost:7879/",
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
    // Findings-enabled instance on :7880 over the synthetic multi-source
    // fixture (zizmor + actionlint SARIF). `browse-findings.spec.ts` hits this
    // server's embedded SPA directly (same-origin API, no Vite proxy) so the
    // cross-cutting FindingsFloat + finding-click flow has automated coverage
    // — the dogfood repo carries no SARIF, so the :7879 path cannot exercise
    // it. The embedded SPA is the freshly built `web/dist` (just frontend &&
    // cargo build --release), matching the live Vite SPA on :5173.
    {
      command:
        "../target/release/ravelact --root ../tests/fixtures/synthetic/zizmor-findings browse --no-open --port 7880 --findings ../tests/fixtures/synthetic/zizmor-findings/zizmor.sarif --findings ../tests/fixtures/synthetic/zizmor-findings/actionlint.sarif",
      url: "http://localhost:7880/",
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
  ],
});
