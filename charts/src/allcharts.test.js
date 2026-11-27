// Every WireCharts chart as a component (RFC 0059): drawn from its docs
// example's props, in the app's look, saying what it shows.
import { afterEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import fixtures from "./test/wirecharts/fixtures.json";
import { charts } from "./test/echarts.js";
import * as main from "./index.js";
import * as gl from "./gl.js";
import { themeOf } from "./internal/theme.js";

const HAND = new Set(["line", "area", "column", "bar", "pie", "scatter"]);
const nameOf = (slug) => slug.split("-").map((w) => w[0].toUpperCase() + w.slice(1)).join("") + "Chart";
const theme = themeOf(null);

afterEach(() => vi.restoreAllMocks());

describe("every WireCharts chart", () => {
  it("is exported, the 3D ones from @elyra/charts/gl", () => {
    for (const f of fixtures) {
      const name = nameOf(f.component);
      expect(main[name] ?? gl[name], name).toBeTruthy();
    }
    expect(Object.keys(gl).sort()).toEqual(["Bar3dChart", "Scatter3dChart", "SurfaceChart"]);
  });

  for (const f of fixtures.filter((x) => !HAND.has(x.component))) {
    const name = nameOf(f.component);
    it(`${name} draws its example and says what it shows`, async () => {
      const Component = main[name] ?? gl[name];
      // The 3D ones draw where there's WebGL.
      if (gl[name]) vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({});
      const props = Object.fromEntries(Object.entries(f.props).filter(([k]) => !["height", "label", "theme"].includes(k)));
      // WireCharts' surface takes `series ?? data`, and `series` defaults to
      // []: its docs' example, which passes `data`, draws nothing there too.
      if (f.component === "surface") Object.assign(props, { series: props.data });
      render(Component, props);
      await tick();
      expect(charts, `${name}: one chart`).toHaveLength(1);
      // The whole option, as first given (a clock or a race then moves its data).
      const option = charts[0].options[0].option;
      // WireCharts' series, in the app's colours and text.
      expect(option.series.map((s) => s.type)).toEqual(f.option.series.map((s) => s.type));
      if (!f.option.color) expect(option.color).toEqual(theme.colors);
      expect(option.textStyle.color).toBe(theme.text);
      // A summary as its name, and its data as a table.
      const group = screen.getByRole("group");
      expect(group.getAttribute("aria-label")?.length, `${name}: a summary`).toBeGreaterThan(0);
      expect(screen.getByRole("table").querySelectorAll("tbody tr").length, `${name}: rows`).toBeGreaterThan(0);
    });
  }
});

describe("3D without WebGL", () => {
  it("says so, and still has its summary and table", async () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    render(gl.Bar3dChart, { series: [[0, 0, 5], [1, 0, 7]], categories: ["A", "B"], rows: ["X"] });
    await tick();
    expect(charts).toHaveLength(0);
    expect(screen.getByText("3D isn't available here")).toBeTruthy();
    expect(screen.getByRole("group").getAttribute("aria-label").length).toBeGreaterThan(0);
    expect(screen.getByRole("table")).toBeTruthy();
  });
});
