// Targets (RFC 0072): a goal for each category, drawn as a tick across a
// column or a step line through a line, and said against the data.
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { CustomChart } from "echarts/charts";
import { phrase } from "@elyra/runtime";
import { charts, use } from "./test/echarts.js";
import { themeOf } from "./internal/theme.js";
import { cartesian } from "./internal/options.js";
import { TARGETS, measuredOf, targetColumns, targetsFrom, targetsOf, targetsSaid, tickOf, tooltipWithTargets } from "./internal/targets.js";
import { WORDS } from "./internal/words.js";
import { BarChart, ColumnChart, LineChart } from "./index.js";

const theme = themeOf(null);
const months = ["Jan", "Feb", "Mar", "Apr"];
const revenue = [{ name: "Revenue", data: [100, 120, 90, 160] }];
const budget = { label: "Budget", data: [110, 110, 100, 150] };
const drawn = (c) => c.options.at(-1).option;
const targetSeries = (option) => option.series.find((s) => s.id === TARGETS);
const words = Object.fromEntries(
  Object.entries(WORDS.charts).map(([name, entry]) => [
    name,
    Array.isArray(entry) ? (...args) => phrase(entry[0], Object.fromEntries(entry.slice(1).map((p, i) => [p, args[i]]))) : entry,
  ]),
);

describe("targets.js", () => {
  it("the app's targets, aligned with the categories; none without a number", () => {
    expect(targetsFrom({ label: "Budget", data: [1, null, 3] }, months)).toEqual({
      label: "Budget",
      data: [1, null, 3, null],
      tone: null,
      lower: false,
    });
    expect(targetsFrom({ data: [null, null] }, months)).toBeNull();
    expect(targetsFrom(undefined, months)).toBeNull();
  });

  it("measured against one series, or a stack's total; not several separate ones", () => {
    const two = [...revenue, { name: "Costs", data: [10, 10, 10, null] }];
    expect(measuredOf(revenue, months)).toEqual([100, 120, 90, 160]);
    expect(measuredOf(two, months, true)).toEqual([110, 130, 100, 160]);
    expect(measuredOf(two, months)).toBeNull();
  });

  it("over columns, a custom series of ticks; over a line, a dashed step line; the axis takes them in", () => {
    const columns = targetsOf(budget, { categories: months, bars: true, theme });
    expect(columns.series[0]).toMatchObject({ id: TARGETS, type: "custom", name: "Budget", silent: true, data: [[0, 110], [1, 110], [2, 100], [3, 150]] });
    expect(columns.extent).toEqual([100, 150]);
    const line = targetsOf(budget, { categories: months, theme });
    expect(line.series[0]).toMatchObject({ type: "line", step: "middle", showSymbol: false, lineStyle: { type: "dashed" }, data: [110, 110, 100, 150] });
  });

  it("a tick is drawn across its band, 70 % of it (at most a column's 48 px), or up a bar", () => {
    const api = { value: (i) => [2, 100][i], coord: ([x, y]) => [x * 100, 300 - y], size: () => [60, 60] };
    expect(tickOf("#f00", false, false)({}, api)).toEqual({ type: "rect", shape: { x: 179, y: 198.5, width: 42, height: 3 }, style: { fill: "#f00" } });
    const sideways = tickOf("#f00", true, false)({}, { ...api, coord: ([x, y]) => [x, y * 10] });
    expect(sideways.shape).toEqual({ x: 98.5, y: -1, width: 3, height: 42 });
    const wide = tickOf("#f00", false, false)({}, { ...api, size: () => [200, 200] });
    expect(wide.shape.width).toBe(48);
  });

  it("said: how many met, at or over, or at or under a ceiling; in a window, only what's shown", () => {
    expect(targetsSaid(budget, { series: revenue, categories: months, words })).toBe("Budget: 2 of 4 met");
    expect(targetsSaid({ ...budget, lower: true }, { series: revenue, categories: months, words })).toBe("Budget: 2 of 4 met");
    expect(targetsSaid({ ...budget, data: [100, 100, 100, 100], lower: true }, { series: revenue, categories: months, words })).toBe("Budget: 2 of 4 met");
    expect(targetsSaid(budget, { series: revenue, categories: months, words, shown: (i) => i < 2 })).toBe("Budget: 1 of 2 met");
    const two = [...revenue, { name: "Costs", data: [1, 1, 1, 1] }];
    expect(targetsSaid(budget, { series: two, categories: months, words })).toBe("Budget shown");
    expect(targetsSaid({ data: [1, 1, 1, 1] }, { series: revenue, categories: months, words })).toBe("Target: 4 of 4 met");
  });

  it("the table's columns: the target and the difference, signed", () => {
    expect(targetColumns(budget, { series: revenue, categories: months, words, locale: "en" })).toEqual({
      columns: ["Budget", "Difference"],
      rows: [["110", "-10"], ["110", "+10"], ["100", "-10"], ["150", "+10"]],
    });
    expect(targetColumns(budget, { series: revenue, categories: months, words, locale: "en", difference: false }).columns).toEqual(["Budget"]);
  });

  it("the tooltip: each series as drawn, the target, and the difference", () => {
    const format = tooltipWithTargets(budget, { series: revenue, categories: months, words, locale: "en" });
    const html = format([
      { axisValueLabel: "Feb", dataIndex: 1, marker: "●", seriesName: "Revenue", value: 120 },
      { dataIndex: 1, marker: "■", seriesName: "Budget", value: [1, 110] },
    ]);
    expect(html).toBe("Feb<br>●Revenue <b>120</b><br>■Budget <b>110</b><br>Difference <b>+10</b>");
  });
});

describe("the option", () => {
  it("a column chart's ticks out of the legend, its tooltip with the difference", () => {
    const series = [...revenue, { name: "Costs", data: [1, 2, 3, 4] }];
    const option = cartesian("column", { series, categories: months }, { stack: true, targets: budget, words }, theme);
    expect(targetSeries(option).type).toBe("custom");
    expect(option.legend.data).toEqual(["Revenue", "Costs"]);
    expect(typeof option.tooltip.formatter).toBe("function");
    expect(option.yAxis).toMatchObject({ dataMin: 100, dataMax: 150 });
  });

  it("a percent stack has no targets; without them the option is as it was", () => {
    expect(targetSeries(cartesian("column", { series: revenue, categories: months }, { percent: true, targets: budget }, theme))).toBeUndefined();
    const plain = cartesian("line", { series: revenue, categories: months }, {}, theme);
    expect(plain.series).toHaveLength(1);
    expect(plain.tooltip.formatter).toBeUndefined();
  });

  it("on a time axis, the targets go with their dates", () => {
    const days = ["2026-03-01", "2026-03-02"];
    const option = cartesian("line", { series: [{ data: [1, 2] }], categories: days }, { targets: { data: [2, 2] } }, theme);
    expect(targetSeries(option).data).toEqual([["2026-03-01", 2], ["2026-03-02", 2]]);
  });
});

describe("a chart with targets", () => {
  it("draws them, with the custom series in use, and says how many met", async () => {
    render(ColumnChart, { series: revenue, categories: months, targets: budget });
    await tick();
    expect(targetSeries(drawn(charts[0]))).toBeTruthy();
    expect(use.mock.calls.some(([parts]) => Array.isArray(parts) && parts.includes(CustomChart))).toBe(true);
    expect(screen.getByRole("group").getAttribute("aria-label").endsWith("Budget: 2 of 4 met")).toBe(true);
  });

  it("the hidden table has the target and the difference", async () => {
    render(LineChart, { series: revenue, categories: months, targets: budget });
    await tick();
    expect([...document.querySelectorAll("thead th")].map((th) => th.textContent)).toEqual(["Category", "Revenue", "Budget", "Difference"]);
    expect([...document.querySelectorAll("tbody tr")][1].textContent).toBe("Feb120110+10");
  });

  it("the CSV has the target beside the data, without the difference", async () => {
    const { component } = render(BarChart, { series: revenue, categories: months, targets: budget, label: "Revenue" });
    await tick();
    expect(component.csv().split("\r\n").slice(1, 3)).toEqual(["Category,Revenue,Budget", "Jan,100,110"]);
  });
});
