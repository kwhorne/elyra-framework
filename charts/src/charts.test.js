// @elyra/charts (RFC 0058): the options each chart gives ECharts, the theme,
// live data and its states, and what the screen reader and the assistant
// read.
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { writable } from "svelte/store";
import { charts } from "./test/echarts.js";
import { speak } from "./test/words.js";
import { cartesian, pie, scatter } from "./internal/options.js";
import { chartData, normalizeSeries, summaryOf, tableOf } from "./internal/data.js";
import { themeOf } from "./internal/theme.js";
import { AreaChart, BarChart, Chart, ColumnChart, LineChart, PieChart, ScatterChart } from "./index.js";

const theme = themeOf(null);
const revenue = { series: [{ name: "Revenue", data: [120, 180, 150, 260] }], categories: ["Jan", "Feb", "Mar", "Apr"] };
const last = () => charts[charts.length - 1].options.at(-1).option;

describe("WireCharts' data", () => {
  it("takes series as WireCharts does: named, a bare list, points", () => {
    expect(normalizeSeries([1, 2, 3])).toEqual([{ data: [1, 2, 3] }]);
    expect(normalizeSeries([{ name: "A", data: [1] }])).toEqual([{ name: "A", data: [1] }]);
    expect(normalizeSeries([{ name: "Open", value: 3 }])).toEqual([{ data: [{ name: "Open", value: 3 }] }]);
    expect(normalizeSeries([[1, 2], [3, 4]])).toEqual([{ data: [[1, 2], [3, 4]] }]);
  });

  it("takes a live value as the series, or with its categories", () => {
    expect(chartData({ series: [], categories: ["a"] }, [1, 2])).toEqual({ series: [{ data: [1, 2] }], categories: ["a"] });
    expect(chartData({ series: [], categories: [] }, revenue)).toEqual(revenue);
  });
});

describe("the options", () => {
  it("a line: a category axis, the series as given, smooth when asked", () => {
    const o = cartesian("line", revenue, { smooth: true }, theme);
    expect(o.xAxis.type).toBe("category");
    expect(o.xAxis.data).toEqual(["Jan", "Feb", "Mar", "Apr"]);
    expect(o.series).toEqual([expect.objectContaining({ type: "line", name: "Revenue", data: [120, 180, 150, 260], smooth: true })]);
    expect(o.color).toEqual(theme.colors);
    expect(o.legend.show).toBe(false);
  });

  it("an area stacked to percent: each category's shares", () => {
    const o = cartesian("area", { series: [{ name: "A", data: [1, 3] }, { name: "B", data: [3, 1] }], categories: ["x", "y"] }, { percent: true }, theme);
    expect(o.series.map((s) => s.data)).toEqual([[25, 75], [75, 25]]);
    expect(o.series.every((s) => s.stack === "total" && s.areaStyle)).toBe(true);
    expect(o.yAxis.max).toBe(100);
    expect(o.legend.show).toBe(true);
  });

  it("a bar chart lies down; a column chart stands", () => {
    expect(cartesian("bar", revenue, {}, theme).yAxis.type).toBe("category");
    expect(cartesian("column", revenue, {}, theme).xAxis.type).toBe("category");
    expect(cartesian("column", revenue, {}, theme).series[0].type).toBe("bar");
  });

  it("ISO dates are a time axis", () => {
    const o = cartesian("line", { series: [{ data: [1, 2] }], categories: ["2026-01-01", "2026-02-01"] }, {}, theme);
    expect(o.xAxis.type).toBe("time");
    expect(o.series[0].data).toEqual([["2026-01-01", 1], ["2026-02-01", 2]]);
  });

  it("a pie names numbers by the categories; a donut leaves the middle open", () => {
    const o = pie({ series: [{ data: [3, 1] }], categories: ["Open", "Closed"] }, { donut: true }, theme);
    expect(o.series[0].data).toEqual([{ name: "Open", value: 3 }, { name: "Closed", value: 1 }]);
    expect(o.series[0].radius).toEqual(["50%", "75%"]);
  });

  it("a scatter sizes bubbles by a third value", () => {
    const o = scatter({ series: [{ name: "P", data: [[1, 2, 4], [3, 4, 16]] }] }, { size: true }, theme);
    expect(o.series[0].symbolSize([1, 2, 16])).toBe(40);
    expect(o.series[0].symbolSize([1, 2, 4])).toBe(24);
  });

  it("no animation when the motion tokens are zero", () => {
    expect(cartesian("line", revenue, {}, { ...theme, duration: 0 }).animation).toBe(false);
  });
});

describe("what it says", () => {
  const words = {
    empty: "No data",
    series: (name, first, last, min, max, count) => `${name}: from ${first} to ${last}, between ${min} and ${max}, over ${count} points`,
    slice: (name, share) => `${name} ${share}`,
  };

  it("a line's summary: where it went", () => {
    expect(summaryOf("line", revenue.series, revenue.categories, undefined, "en", words)).toBe(
      "Revenue: from 120 to 260, between 120 and 260, over 4 points",
    );
  });

  it("a pie's: the largest slices and their shares", () => {
    const s = normalizeSeries([{ data: [{ name: "Open", value: 3 }, { name: "Closed", value: 1 }] }]);
    expect(summaryOf("pie", s, [], undefined, "en", words)).toBe("Open 75%, Closed 25%");
  });

  it("the table: a row per category, a column per series", () => {
    expect(tableOf("line", revenue.series, revenue.categories, undefined, "en")).toEqual({
      columns: ["Revenue"],
      rows: [["Jan", ["120"]], ["Feb", ["180"]], ["Mar", ["150"]], ["Apr", ["260"]]],
    });
  });
});

describe("the components", () => {
  it("draw with their own ECharts part, in SVG, and say what they show", async () => {
    render(LineChart, { ...revenue });
    await tick();
    expect(charts).toHaveLength(1);
    expect(charts[0].opts).toEqual({ renderer: "svg" });
    expect(last().series[0].type).toBe("line");
    const chart = screen.getByRole("group", { name: "Revenue: from 120 to 260, between 120 and 260, over 4 points" });
    expect(chart.getAttribute("aria-roledescription")).toBe("chart");
    expect(screen.getByRole("table", { name: "The chart's data" })).toBeTruthy();
    expect(screen.getByRole("cell", { name: "260" })).toBeTruthy();
  });

  for (const [Component, type] of [
    [AreaChart, "line"],
    [ColumnChart, "bar"],
    [BarChart, "bar"],
    [PieChart, "pie"],
  ]) {
    it(`${Component.name ?? type} draws ${type}`, async () => {
      render(Component, { ...revenue });
      await tick();
      expect(last().series[0].type).toBe(type);
    });
  }

  it("ScatterChart draws points", async () => {
    render(ScatterChart, { series: [{ name: "P", data: [[1, 2], [3, 4]] }] });
    await tick();
    expect(last().series[0].type).toBe("scatter");
  });

  it("Chart takes any option, over the theme", async () => {
    render(Chart, { option: { series: [{ type: "heatmap", data: [[0, 0, 1]] }] }, label: "Load by hour" });
    await tick();
    expect(last().series[0].type).toBe("heatmap");
    expect(last().color).toEqual(theme.colors);
    expect(screen.getByRole("group", { name: "Load by hour" })).toBeTruthy();
  });

  it("follow a live query: a skeleton, then the chart, moved to new data", async () => {
    const store = writable({ value: undefined, error: null, loading: true });
    const { container } = render(ColumnChart, { live: store });
    await tick();
    expect(container.querySelector(".elyra-skeleton")).toBeTruthy();
    expect(charts).toHaveLength(0);
    store.set({ value: revenue, error: null, loading: false });
    await tick();
    expect(charts).toHaveLength(1);
    store.set({ value: { ...revenue, series: [{ name: "Revenue", data: [1, 2, 3, 4] }] }, error: null, loading: false });
    await tick();
    expect(charts).toHaveLength(1);
    const { option, how } = charts[0].options.at(-1);
    expect(option.series[0].data).toEqual([1, 2, 3, 4]);
    expect(how).toEqual({ replaceMerge: ["series"] });
  });

  it("say why there's no chart: an error, or no data", async () => {
    render(LineChart, { live: writable({ value: undefined, error: new Error("Server down"), loading: false }) });
    await tick();
    expect(screen.getByText("Couldn't load this chart")).toBeTruthy();
    expect(screen.getByText("Server down")).toBeTruthy();
  });

  it("an empty chart says so", async () => {
    render(PieChart, { series: [] });
    await tick();
    expect(screen.getByText("No data")).toBeTruthy();
    expect(charts).toHaveLength(0);
  });

  it("←/→ walk the categories and Enter selects", async () => {
    const chosen = [];
    render(ColumnChart, { ...revenue, onselect: (s) => chosen.push(s) });
    await tick();
    const chart = screen.getByRole("group");
    chart.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    chart.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }));
    chart.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(charts[0].actions.at(-1)).toEqual({ type: "showTip", seriesIndex: 0, dataIndex: 1 });
    expect(chosen).toEqual([{ series: "Revenue", category: "Feb", value: 180 }]);
  });

  it("a click on a point is a selection too", async () => {
    const chosen = [];
    render(LineChart, { ...revenue, onselect: (s) => chosen.push(s) });
    await tick();
    charts[0].handlers.click({ seriesName: "Revenue", name: "Mar", value: 150 });
    expect(chosen).toEqual([{ series: "Revenue", category: "Mar", value: 150 }]);
  });

  it("speak the app's language, and format in its locale", async () => {
    speak("nb", {
      "elyra.charts.caption": "Dataene i diagrammet",
      "elyra.charts.series": "{1} :name: :first|[2,*] :name: fra :first til :last, mellom :min og :max, over :count punkter",
    });
    render(LineChart, { series: [{ name: "Omsetning", data: [1200.5, 2600] }], categories: ["jan", "feb"] });
    await tick();
    // Norwegian groups thousands with a no-break space.
    const n = (x) => new Intl.NumberFormat("nb", { maximumFractionDigits: 2 }).format(x);
    const name = `Omsetning: fra ${n(1200.5)} til ${n(2600)}, mellom ${n(1200.5)} og ${n(2600)}, over 2 punkter`;
    expect(screen.getByRole("group", { name })).toBeTruthy();
    expect(screen.getByRole("table", { name: "Dataene i diagrammet" })).toBeTruthy();
  });

  it("are disposed with the component", async () => {
    const { unmount } = render(LineChart, { ...revenue });
    await tick();
    unmount();
    expect(charts[0].disposed).toBe(true);
  });
});

describe("the words", () => {
  it("are framework/lang/en.json's, and all in Norwegian", async () => {
    const { readFileSync } = await import("node:fs");
    const { WORDS } = await import("./internal/words.js");
    const lang = (l) => JSON.parse(readFileSync(`${import.meta.dirname}/../../framework/lang/${l}.json`, "utf8")).elyra.charts;
    const english = Object.fromEntries(Object.entries(WORDS.charts).map(([k, v]) => [k, Array.isArray(v) ? v[0] : v]));
    expect(lang("en")).toEqual(english);
    expect(Object.keys(lang("nb")).sort()).toEqual(Object.keys(english).sort());
  });
});
