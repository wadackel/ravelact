import { defineConfig } from "vitest/config";

export default defineConfig({
  // Mirror the production build-time injection from vite.config.ts so unit
  // tests stay decoupled from Cargo.toml. The literal `'"0.0.0-test"'` is
  // intentional: `define` performs raw text substitution, so the value must
  // include its own outer quotes to be a valid string expression in the
  // compiled test bundle.
  define: {
    __RAVELACT_VERSION__: '"0.0.0-test"',
  },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    setupFiles: ["src/test-setup.ts"],
    globals: true,
  },
});
