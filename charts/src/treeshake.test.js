// Tree-shaken (RFC 0058): an app that draws only a pie ships ECharts' pie,
// not its line, bar or scatter.
import { describe, expect, it } from "vitest";
import { build } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

async function bundle(source) {
  const dir = mkdtempSync(join(tmpdir(), "elyra-charts-"));
  try {
    writeFileSync(join(dir, "main.js"), source);
    const out = await build({
      root: dir,
      logLevel: "silent",
      configFile: false,
      plugins: [svelte()],
      resolve: { conditions: ["browser"], dedupe: ["svelte", "@elyra/runtime"] },
      build: {
        write: false,
        minify: true,
        lib: { entry: join(dir, "main.js"), formats: ["es"], fileName: "app" },
        rollupOptions: { external: [/^@elyra\/(ui|runtime)/, /^svelte/] },
      },
    });
    const outputs = (Array.isArray(out) ? out : [out]).flatMap((o) => o.output);
    return outputs.map((o) => o.code ?? "").join("\n");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

describe("the bundle", () => {
  it("has only the charts the app draws", async () => {
    const charts = `${import.meta.dirname}/index.js`;
    const pieOnly = await bundle(`import { PieChart } from ${JSON.stringify(charts)}; export { PieChart };`);
    expect(pieOnly).toContain("series.pie");
    expect(pieOnly).not.toContain("series.line");
    expect(pieOnly).not.toContain("series.bar");
    expect(pieOnly).not.toContain("series.scatter");
  }, 60_000);

  it("has one WireCharts chart's parts, and no 3D unless asked", async () => {
    const charts = `${import.meta.dirname}/index.js`;
    const funnel = await bundle(`import { FunnelChart } from ${JSON.stringify(charts)}; export { FunnelChart };`);
    expect(funnel).toContain("funnelAlign");
    for (const other of ["series.line", "series.sankey", "series.heatmap", "series.gauge"]) expect(funnel).not.toContain(other);
    // No echarts-gl: the 3D charts are @elyra/charts/gl's alone.
    expect(funnel).not.toMatch(/WebGL layer|claygl/i);
  }, 60_000);
});
