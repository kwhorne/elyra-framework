// Marks (RFC 0065): a target, a threshold, a period; drawn by series of
// their own, taken in by the value axis, and said.
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { phrase } from "@elyra/runtime";
import { MarkLineComponent } from "echarts/components";
import { charts, use } from "./test/echarts.js";
import { themeOf } from "./internal/theme.js";
import { withAlpha } from "./internal/brush.js";
import { bandRows, marksFrom, marksOf, marksSaid, MARK_AREAS, MARK_LINES } from "./internal/marks.js";
import { cartesian, scatter } from "./internal/options.js";
import { WORDS } from "./internal/words.js";
import { BarChart, ColumnChart, LineChart, ScatterChart } from "./index.js";

const theme = themeOf(null);
const months = ["Jan", "Feb", "Mar", "Apr", "May"];
const revenue = [{ name: "Revenue", data: [100, 120, 150, 260, 200] }];
const drawn = (c) => c.options.at(-1).option;
const byId = (option, id) => option.series.find((s) => s.id === id);
// The English words, as a chart has them without a catalog.
const words = Object.fromEntries(
  Object.entries(WORDS.charts).map(([name, entry]) => [
    name,
    Array.isArray(entry) ? (...args) => phrase(entry[0], Object.fromEntries(entry.slice(1).map((p, i) => [p, args[i]]))) : entry,
  ]),
);

describe("marksFrom", () => {
  it("a line or a band on either axis; the rest is dropped", () => {
    expect(
      marksFrom([
        { value: 120, label: "Budget" },
        { value: [120, 80], tone: "success" },
        { at: "Mar" },
        { at: ["Jun", "Aug"], tone: "nope" },
        { value: "high" },
        { label: "Nothing" },
        null,
      ]),
    ).toEqual([
      { on: "value", from: 120, to: 120, band: false, label: "Budget", tone: "muted" },
      { on: "value", from: 80, to: 120, band: true, label: null, tone: "success" },
      { on: "at", from: "Mar", to: "Mar", band: false, label: null, tone: "muted" },
      { on: "at", from: "Jun", to: "Aug", band: true, label: null, tone: "muted" },
    ]);
    expect(marksFrom(undefined)).toEqual([]);
  });
});

describe("marksOf", () => {
  it("lines over the data, bands behind it, each in its tone", () => {
    const { series, extent } = marksOf(
      [
        { value: 120, label: "Budget" },
        { value: [80, 110], label: "Normal", tone: "success" },
        { at: "Mar", tone: "accent" },
        { at: ["Feb", "Apr"], label: "Campaign" },
      ],
      { categories: months, theme },
    );
    const areas = series.find((s) => s.id === MARK_AREAS);
    const lines = series.find((s) => s.id === MARK_LINES);
    expect(areas.z).toBeLessThan(2);
    expect(lines.z).toBeGreaterThan(2);
    for (const s of series) expect(s).toMatchObject({ type: "line", data: [], silent: true, tooltip: { show: false } });
    expect(lines.markLine.data).toMatchObject([
      { yAxis: 120, lineStyle: { color: theme.muted, type: "dashed", width: 1 }, label: { show: true } },
      { xAxis: 2, lineStyle: { color: theme.accent }, label: { show: false } },
    ]);
    expect(lines.markLine.data[0].label.formatter()).toBe("Budget");
    expect(areas.markArea.data).toMatchObject([
      [{ yAxis: 80, itemStyle: { color: withAlpha(theme.success, 0.12) } }, { yAxis: 110 }],
      [{ xAxis: 1 }, { xAxis: 3 }],
    ]);
    expect(extent).toEqual([80, 120]);
  });

  it("in a BarChart the axes are turned, and so are the marks", () => {
    const { series } = marksOf([{ value: 50 }, { at: "Feb" }], { categories: months, sideways: true, theme });
    expect(series[0].markLine.data).toMatchObject([{ xAxis: 50 }, { yAxis: 1 }]);
  });

  it("a band over bars takes in the bars at its ends", () => {
    const { series } = marksOf([{ at: ["Feb", "Apr"] }], { categories: months, bars: true, theme });
    expect(series[0].markArea.data[0]).toMatchObject([{ xAxis: 0.5 }, { xAxis: 3.5 }]);
  });

  it("a date that isn't a category is the nearest one; on a time axis, its time", () => {
    const days = ["2026-03-01", "2026-03-08", "2026-03-15"];
    expect(marksOf([{ at: "2026-03-09" }], { categories: days, theme }).series[0].markLine.data[0].xAxis).toBe(1);
    expect(marksOf([{ at: "2026-03-09" }], { categories: days, axisType: "time", theme }).series[0].markLine.data[0].xAxis).toBe(
      Date.parse("2026-03-09"),
    );
    // A category that isn't there, and isn't a date, isn't drawn.
    expect(marksOf([{ at: "Dec" }], { categories: months, theme }).series).toEqual([]);
  });

  it("no marks, nothing added", () => {
    expect(marksOf(undefined, { categories: months, theme })).toEqual({ series: [], extent: null });
  });
});

describe("the option", () => {
  it("a value axis that takes in a mark beyond the data, and a legend of the data only", () => {
    const series = [...revenue, { name: "Costs", data: [50, 60, 70, 80, 90] }];
    const option = cartesian("column", { series, categories: months }, { marks: [{ value: 400, label: "Budget" }] }, theme);
    expect(option.yAxis).toMatchObject({ type: "value", dataMin: 400, dataMax: 400 });
    expect(option.yAxis.min).toBeUndefined();
    expect(option.series.map((s) => s.id ?? s.name)).toEqual(["Revenue", "Costs", MARK_LINES]);
    expect(option.legend.data).toEqual(["Revenue", "Costs"]);
    // A stacked chart's stack isn't touched.
    const stacked = cartesian("column", { series, categories: months }, { stack: true, marks: [{ value: 400 }] }, theme);
    expect(byId(stacked, MARK_LINES).stack).toBeUndefined();
  });

  it("a BarChart's value axis is x; a scatter's marks take in both axes", () => {
    const bar = cartesian("bar", { series: revenue, categories: months }, { marks: [{ value: 400 }] }, theme);
    expect(bar.xAxis).toMatchObject({ type: "value", dataMax: 400 });
    const points = scatter({ series: [{ data: [[1, 2], [3, 4]] }] }, { marks: [{ value: 10 }, { at: [-5, 2] }] }, theme);
    expect(points.yAxis).toMatchObject({ dataMin: 10, dataMax: 10 });
    expect(points.xAxis).toMatchObject({ dataMin: -5, dataMax: 2 });
    expect(byId(points, MARK_AREAS).markArea.data[0]).toMatchObject([{ xAxis: -5 }, { xAxis: 2 }]);
  });

  it("without marks, the option is as it was", () => {
    const option = cartesian("line", { series: revenue, categories: months }, {}, theme);
    expect(option.series).toHaveLength(1);
    expect(option.yAxis.dataMax).toBeUndefined();
    expect(option.legend.data).toBeUndefined();
  });
});

describe("marksSaid", () => {
  const base = { kind: "column", series: revenue, categories: months, words };

  it("a value line with the points over it; a band with those in it", () => {
    expect(marksSaid([{ value: 140, label: "Budget" }, { value: [100, 150], label: "Normal" }], base)).toEqual([
      "Budget 140: 3 of 5 over",
      "Normal 100 to 150: 3 of 5 within",
    ]);
  });

  it("a stack is counted by its totals; separate series name the line only", () => {
    const two = [...revenue, { name: "Costs", data: [50, 50, 50, 50, 50] }];
    expect(marksSaid([{ value: 200, label: "Budget" }], { ...base, series: two, stack: true })).toEqual(["Budget 200: 2 of 5 over"]);
    expect(marksSaid([{ value: 200, label: "Budget" }], { ...base, series: two })).toEqual(["Budget 200"]);
    expect(marksSaid([{ value: [10, 90] }], { ...base, series: two })).toEqual(["Mark 10 to 90"]);
  });

  it("a line at a category, a period", () => {
    expect(marksSaid([{ at: "Mar", label: "Price change" }, { at: ["Feb", "Apr"], label: "Campaign" }], base)).toEqual([
      "Price change at Mar",
      "Campaign from Feb to Apr",
    ]);
  });

  it("in a window: what's shown is counted, and a mark outside it isn't said", () => {
    const axis = { type: "category", values: months, min: 0, max: 4 };
    const shown = (i) => i <= 1;
    expect(
      marksSaid([{ value: 110, label: "Budget" }, { at: "May", label: "Launch" }, { at: "Feb", label: "Sale" }], {
        ...base,
        axis,
        range: ["Jan", "Feb"],
        shown,
      }),
    ).toEqual(["Budget 110: 1 of 2 over", null, "Sale at Feb"]);
  });

  it("a scatter: its x, and its points' y", () => {
    const points = [{ data: [[1, 5], [2, 15], [3, 25]] }];
    expect(marksSaid([{ value: 10, label: "Limit" }, { at: 2, label: "Cut" }], { kind: "scatter", series: points, categories: [], words })).toEqual([
      "Limit 10: 2 of 3 over",
      "Cut at 2",
    ]);
  });
});

describe("bandRows", () => {
  it("the categories each period covers", () => {
    const rows = bandRows([{ value: 3 }, { at: ["Feb", "Apr"] }, { at: "May" }], { categories: months });
    expect(rows.map(([i, set]) => [i, [...set]])).toEqual([[1, [1, 2, 3]]]);
  });
});

describe("a chart with marks", () => {
  it("draws them with the mark components, and says them", async () => {
    render(ColumnChart, { series: revenue, categories: months, marks: [{ value: 140, label: "Budget" }] });
    await tick();
    const [chart] = charts;
    expect(byId(drawn(chart), MARK_LINES).markLine.data[0].yAxis).toBe(140);
    expect(use.mock.calls.some(([parts]) => Array.isArray(parts) && parts.includes(MarkLineComponent))).toBe(true);
    const label = screen.getByRole("group").getAttribute("aria-label");
    expect(label.endsWith("Budget 140: 3 of 5 over")).toBe(true);
  });

  it("the table: a sentence each, and the rows a period covers point at its own", async () => {
    render(LineChart, { series: revenue, categories: months, marks: [{ at: ["Feb", "Mar"], label: "Campaign" }] });
    await tick();
    const sentence = screen.getByText("Campaign from Feb to Mar");
    expect(sentence.id).toBeTruthy();
    const rows = [...document.querySelectorAll("tbody th")];
    expect(rows.map((th) => th.getAttribute("aria-describedby"))).toEqual([null, sentence.id, sentence.id, null, null]);
  });

  it("a BarChart and a scatter take them too; new marks are drawn", async () => {
    const bar = render(BarChart, { series: revenue, categories: months, marks: [{ value: 300 }] });
    render(ScatterChart, { series: [{ data: [[1, 2], [3, 4]] }], marks: [{ at: 2, label: "Cut" }] });
    await tick();
    const [a, b] = charts;
    expect(drawn(a).xAxis.dataMax).toBe(300);
    expect(byId(drawn(b), MARK_LINES).markLine.data[0].xAxis).toBe(2);
    await bar.rerender({ series: revenue, categories: months, marks: [{ value: 500 }] });
    await tick();
    expect(drawn(charts.at(-1)).xAxis.dataMax).toBe(500);
  });
});
