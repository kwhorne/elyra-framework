import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  // Svelte's browser build, not its server one: the components render in a DOM.
  resolve: { conditions: ["browser"] },
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.js"],
    setupFiles: ["src/test-setup.js"],
  },
});
