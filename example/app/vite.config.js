import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// `base: "./"` makes the build emit relative asset URLs, which is required
// because the app is served from `elyra://localhost/` (not a web root).
export default defineConfig({
  plugins: [svelte()],
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "esnext",
  },
  server: {
    port: 5173,
    strictPort: true,
  },
});
