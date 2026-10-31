import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

// `npm run docs` (dev) / `npm run docs:build`: the reference, docs/ui.md and
// docs/ui/*.md, with every example running.
export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [svelte()],
  resolve: {
    alias: {
      // The examples import the package as an app does.
      "@elyra/ui": fileURLToPath(new URL("../src/index.js", import.meta.url)),
      "@elyra/runtime": fileURLToPath(new URL("./runtime.js", import.meta.url)),
    },
  },
  // The pages are the repository's docs/.
  server: { fs: { allow: [fileURLToPath(new URL("../..", import.meta.url))] } },
  build: { outDir: "dist", emptyOutDir: true, assetsInlineLimit: 100000, cssCodeSplit: false },
});
