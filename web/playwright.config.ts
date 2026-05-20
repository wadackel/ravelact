import { defineConfig } from "@playwright/test";

// Two-server harness:
//
//   1. Vite dev server on :5173 with `/api/*` proxied to ravelact at :7879
//      (set via the `RAVELACT_PORT` env var read in `vite.config.ts`).
//   2. ravelact `browse` on :7879 — separate from the dev workflow's :7878
//      so a developer running `pnpm dev` in another shell does not collide
//      when `pnpm e2e` is invoked. `reuseExistingServer: true` lets a
//      pre-warmed instance be reused locally.
//
// In CI the binary must already exist at `../target/release/ravelact`
// (produced by `just frontend && cargo build --release`, see justfile).

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
      url: "http://localhost:7879/api/graph",
      reuseExistingServer: !process.env.CI,
      timeout: 30_000,
    },
  ],
});
