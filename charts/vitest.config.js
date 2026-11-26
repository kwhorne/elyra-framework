import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  // Svelte's browser build, not its server one: the charts render in a DOM.
  // One Svelte and one runtime, though @elyra/ui is linked with its own.
  resolve: { conditions: ["browser"], dedupe: ["svelte", "@elyra/runtime"] },
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.js"],
    setupFiles: ["src/test-setup.js"],
  },
});
