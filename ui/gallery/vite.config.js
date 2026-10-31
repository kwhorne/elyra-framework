import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

// `npm run gallery` (dev) / `npm run gallery:build`: every component, in
// both themes, for a person to look at.
export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [svelte()],
  resolve: {
    alias: { "@elyra/runtime": fileURLToPath(new URL("./runtime.js", import.meta.url)) },
  },
  build: { outDir: "dist", emptyOutDir: true, assetsInlineLimit: 100000, cssCodeSplit: false },
});
