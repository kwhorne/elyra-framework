import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

// `npm run docs` / `npm run docs:build`: every chart, drawn from WireCharts'
// docs examples (the fixtures the tests use), with its code.
export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [svelte()],
  resolve: {
    conditions: ["browser"],
    dedupe: ["svelte", "echarts"],
    alias: {
      "@elyra/runtime": fileURLToPath(new URL("./runtime.js", import.meta.url)),
    },
  },
  server: { fs: { allow: [fileURLToPath(new URL("../..", import.meta.url))] } },
  build: { outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 4000 },
});
