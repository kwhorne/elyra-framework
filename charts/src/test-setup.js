import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/svelte";

// ECharts measures and paints; the tests read the options it was given.
vi.mock("echarts/core", async () => await import("./test/echarts.js"));
// echarts-gl draws with WebGL, which the tests don't have.
vi.mock("echarts-gl/charts", () => ({ Bar3DChart: {}, Scatter3DChart: {}, SurfaceChart: {} }));
vi.mock("echarts-gl/components", () => ({ Grid3DComponent: {} }));

// The app's catalog: none, unless a test speaks one.
vi.mock("@elyra/runtime", async (importOriginal) => {
  const real = await importOriginal();
  const words = await import("./test/words.js");
  const system = await import("./test/system.js");
  return {
    ...real,
    frameworkWords: words.frameworkWords,
    paths: system.paths,
    files: system.files,
    clipboard: system.clipboard,
    toast: system.toast,
  };
});

beforeEach(async () => {
  (await import("./test/echarts.js")).reset();
  (await import("./test/words.js")).speak("", {});
  (await import("./test/system.js")).reset();
});

afterEach(() => cleanup());
