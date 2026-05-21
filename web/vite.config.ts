import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// `RAVELACT_PORT` lets playwright (or any harness) point Vite's dev proxy at a
// different ravelact instance without editing this file. Defaults to :7878 to
// match the README's recommended dev workflow.
const ravelactPort = Number(process.env.RAVELACT_PORT ?? 7878);

// oxfmt configuration lives in .oxfmtrc.json (vp's --init defaults to
// injecting `fmt:` into vite.config.ts but vite-plus@0.1.22's defineConfig
// types break our Plugin<any>[] inference, so we keep the standalone config).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: `http://127.0.0.1:${ravelactPort}`,
        changeOrigin: false,
      },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // Deterministic hash pattern. Vite 4+ has known non-deterministic hash
    // edge cases when output filenames are left to the rollup defaults; an
    // explicit pattern keeps the source hash stable across repeat builds and
    // makes the rust-embed pipeline reproducible.
    rollupOptions: {
      output: {
        entryFileNames: "assets/[name]-[hash].js",
        chunkFileNames: "assets/[name]-[hash].js",
        assetFileNames: "assets/[name]-[hash][extname]",
      },
    },
  },
});
