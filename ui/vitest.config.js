import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

export default defineConfig({
  plugins: [svelte()],
  // Svelte's browser build, not its server one: the components render in a DOM.
  resolve: {
    conditions: ["browser"],
    // The reference's examples import the package as an app does.
    alias: { "@elyra/ui": fileURLToPath(new URL("./src/index.js", import.meta.url)) },
  },
  test: {
    environment: "happy-dom",
    include: ["src/**/*.test.js"],
    setupFiles: ["src/test-setup.js"],
  },
});
