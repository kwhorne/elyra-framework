// Zoom (RFC 0061): a window on a long series, as the axis's values; the
// keys and the slider's handles; what the chart says of it; live data;
// kept with `persist`.
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { writable } from "svelte/store";
import { charts } from "./test/echarts.js";
import { saved, store } from "./test/system.js";
import ZoomHarness from "./test/ZoomHarness.svelte";
import { cartesian, scatter } from "./internal/options.js";
import { themeOf } from "./internal/theme.js";
import { axisOf, follow, rangeOf, windowOf, zoomBy } from "./internal/zoom.js";
import { BarChart, Chart, LineChart, LineTimeChart, ScatterChart } from "./index.js";

const theme = themeOf(null);
const months = ["Jan", "Feb", "Mar", "Apr", "May"];
const sales = { series: [{ name: "Revenue", data: [100, 120, 150, 260, 200] }], categories: months };
const chart = () => charts[charts.length - 1];
const drawn = () => chart().options.at(-1).option;
const bound = () => JSON.parse(document.querySelector("output").textContent);
const key = (el, k, more = {}) => el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, ...more }));
const lastZoom = () => chart().actions.filter((a) => a.type === "dataZoom").at(-1);

describe("the option", () => {
  it("zoom adds the wheel's and the slider's data zoom on the category axis; none without", () => {
    const o = cartesian("line", sales, { zoom: true }, theme);
    expect(o.dataZoom).toEqual([{ type: "inside", xAxisIndex: 0 }, { type: "slider", xAxisIndex: 0 }]);
    expect(o.grid.bottom).toBe(44);
    expect(cartesian("line", sales, {}, theme).dataZoom).toBeUndefined();
    expect(cartesian("bar", sales, { zoom: true }, theme).dataZoom[1]).toEqual({ type: "slider", yAxisIndex: 0 });
    expect(scatter({ series: [{ data: [[1, 2]] }] }, { zoom: true }, theme).dataZoom[0]).toEqual({ type: "inside", xAxisIndex: 0 });
  });

  it("the slider in the tokens' colours, where the handles over it are", async () => {
    render(LineChart, { ...sales, zoom: true });
    await tick();
    const slider = drawn().dataZoom[1];
    expect(slider).toMatchObject({ fillerColor: theme.accentSoft, backgroundColor: theme.inset, left: 48, right: 48, bottom: 8, height: 22 });
    expect(slider.handleStyle.color).toBe(theme.accent);
    expect(drawn().dataZoom[0]).toMatchObject({ type: "inside", start: 0, end: 100 });
  });
});

describe("the window as the axis's values", () => {
  const axis = axisOf(cartesian("line", sales, { zoom: true }, theme));

  it("a category axis: its categories, to percents and back", () => {
    expect(windowOf(axis, ["Feb", "Apr"])).toEqual({ start: 25, end: 75 });
    expect(rangeOf(axis, { start: 25, end: 75 })).toEqual(["Feb", "Apr"]);
    expect(rangeOf(axis, { start: 0, end: 100 })).toBeNull();
    expect(windowOf(axis, null)).toEqual({ start: 0, end: 100 });
  });

  it("a time axis: the points' dates, as they were given", () => {
    const days = ["2026-03-01", "2026-03-02", "2026-03-03", "2026-03-04", "2026-03-05"];
    const time = axisOf(cartesian("line", { series: [{ data: [1, 2, 3, 4, 5] }], categories: days }, { zoom: true }, theme));
    expect(time.type).toBe("time");
    expect(windowOf(time, ["2026-03-02", "2026-03-04"])).toEqual({ start: 25, end: 75 });
    expect(rangeOf(time, { start: 20, end: 80 })).toEqual(["2026-03-02", "2026-03-04"]);
  });

  it("a scatter's x: numbers", () => {
    const xs = axisOf(scatter({ series: [{ data: [[0, 1], [10, 2], [20, 3]] }] }, { zoom: true }, theme));
    expect(rangeOf(xs, { start: 25, end: 50 })).toEqual([5, 10]);
  });

  it("new data: a window at the end follows it, one in the middle keeps its values", () => {
    const more = axisOf(cartesian("line", { ...sales, categories: [...months, "Jun"] }, { zoom: true }, theme));
    const end = follow(axis, more, windowOf(axis, ["Apr", "May"]), ["Apr", "May"]);
    expect(rangeOf(more, end)).toEqual(["May", "Jun"]);
    const middle = follow(axis, more, windowOf(axis, ["Feb", "Mar"]), ["Feb", "Mar"]);
    expect(rangeOf(more, middle)).toEqual(["Feb", "Mar"]);
  });

  it("zooming in keeps the focused point where it is, and never below two categories", () => {
    expect(zoomBy(axis, { start: 0, end: 100 }, 0.5, 50)).toEqual({ start: 25, end: 75 });
    expect(zoomBy(axis, { start: 25, end: 50 }, 0.5)).toEqual({ start: 25, end: 50 });
  });
});

describe("bind:range", () => {
  it("in: the window drawn from the start, and moved when the app sets it", async () => {
    const { component } = render(ZoomHarness, { ...sales, range: ["Feb", "Apr"] });
    await tick();
    expect(drawn().dataZoom[1]).toMatchObject({ start: 25, end: 75 });
    component.set(["Mar", "May"]);
    await tick();
    expect(lastZoom()).toEqual({ type: "dataZoom", start: 50, end: 100 });
    component.set(null);
    await tick();
    expect(lastZoom()).toEqual({ type: "dataZoom", start: 0, end: 100 });
  });

  it("out: the wheel, the slider and a value zoom say where the window is", async () => {
    render(ZoomHarness, sales);
    await tick();
    expect(bound()).toBeNull();
    chart().handlers.datazoom({ start: 0, end: 50 });
    await tick();
    expect(bound()).toEqual(["Jan", "Mar"]);
    chart().handlers.datazoom({ batch: [{ start: 75, end: 100 }] });
    await tick();
    expect(bound()).toEqual(["Apr", "May"]);
    chart().handlers.datazoom({ batch: [{ startValue: 1, endValue: 2 }] });
    await tick();
    expect(bound()).toEqual(["Feb", "Mar"]);
    chart().handlers.datazoom({ start: 0, end: 100 });
    await tick();
    expect(bound()).toBeNull();
  });

  it("live data keeps the window, and one at the end follows it", async () => {
    const live = writable({ value: sales, error: null, loading: false });
    render(ZoomHarness, { live, range: ["Apr", "May"] });
    await tick();
    live.set({ value: { ...sales, categories: [...months, "Jun"], series: [{ name: "Revenue", data: [1, 2, 3, 4, 5, 6] }] }, error: null, loading: false });
    await tick();
    expect(bound()).toEqual(["May", "Jun"]);
    expect(drawn().dataZoom[1]).toMatchObject({ start: 80, end: 100 });
  });
});

describe("the keyboard", () => {
  it("+ and - zoom about the focused category, 0 shows all, Shift+←/→ pan", async () => {
    render(ZoomHarness, sales);
    await tick();
    const group = screen.getByRole("group");
    key(group, "ArrowRight");
    key(group, "ArrowRight");
    key(group, "+");
    await tick();
    // Feb is at 25 %: half the width about it.
    expect(lastZoom()).toEqual({ type: "dataZoom", start: 12.5, end: 62.5 });
    key(group, "ArrowRight", { shiftKey: true });
    await tick();
    expect(lastZoom()).toEqual({ type: "dataZoom", start: 37.5, end: 87.5 });
    key(group, "0");
    await tick();
    expect(lastZoom()).toEqual({ type: "dataZoom", start: 0, end: 100 });
    expect(bound()).toBeNull();
    key(group, "+");
    key(group, "-");
    await tick();
    expect(lastZoom()).toEqual({ type: "dataZoom", start: 0, end: 100 });
  });

  it("←/→ past the window's edge move it", async () => {
    render(ZoomHarness, { ...sales, range: ["Jan", "Feb"] });
    await tick();
    const group = screen.getByRole("group");
    key(group, "ArrowRight");
    key(group, "ArrowRight");
    key(group, "ArrowRight");
    await tick();
    expect(chart().actions.at(-1)).toEqual({ type: "showTip", seriesIndex: 0, dataIndex: 2 });
    expect(bound()).toEqual(["Feb", "Mar"]);
  });

  it("the slider's handles: in the tab order, named, with their values, moved by the keys", async () => {
    render(ZoomHarness, { ...sales, range: ["Feb", "Apr"] });
    await tick();
    const [start, end] = screen.getAllByRole("slider");
    expect(start.getAttribute("aria-label")).toBe("Start of the window");
    expect(start.getAttribute("aria-valuetext")).toBe("Feb");
    expect(end.getAttribute("aria-valuetext")).toBe("Apr");
    expect(start.tabIndex).toBe(0);
    key(start, "ArrowLeft");
    await tick();
    expect(bound()).toEqual(["Jan", "Apr"]);
    key(end, "End");
    await tick();
    expect(bound()).toBeNull();
    expect(screen.getByRole("group").getAttribute("aria-label")).not.toContain("–");
  });

  it("a bar chart's slider stands up", async () => {
    render(ZoomHarness, { ...sales, chart: BarChart });
    await tick();
    expect(drawn().dataZoom[1]).toMatchObject({ orient: "vertical", yAxisIndex: 0, right: 8 });
    expect(screen.getAllByRole("slider")[0].getAttribute("aria-orientation")).toBe("vertical");
  });
});

describe("what it says", () => {
  it("the summary is of the window; the table keeps every row and marks the shown", async () => {
    render(ZoomHarness, { ...sales, range: ["Mar", "Apr"] });
    await tick();
    const group = screen.getByRole("group");
    expect(group.getAttribute("aria-label")).toBe("Mar – Apr: Revenue: from 150 to 260, between 150 and 260, over 2 points");
    expect(document.querySelector("caption").textContent).toBe("The chart's data, Mar – Apr shown");
    const rows = [...document.querySelectorAll("tbody th")];
    expect(rows).toHaveLength(5);
    expect(rows.map((r) => r.getAttribute("aria-current"))).toEqual([null, null, "true", "true", null]);
  });

  it("without a window, as before: no handles, no marks", async () => {
    render(LineChart, sales);
    await tick();
    expect(screen.queryAllByRole("slider")).toHaveLength(0);
    expect(document.querySelector("caption").textContent).toBe("The chart's data");
    expect(drawn().dataZoom).toBeUndefined();
  });

  it("a scatter's window is of its x", async () => {
    render(ZoomHarness, { chart: ScatterChart, series: [{ name: "Fit", data: [[0, 1], [10, 2], [20, 3]] }], range: [5, 20] });
    await tick();
    expect(screen.getByRole("group").getAttribute("aria-label")).toBe("5 – 20: Fit: from 2 to 3, between 2 and 3, over 2 points");
  });

  it("WireCharts' time line: its dates, its summary of the window", async () => {
    const series = [{ name: "Visits", data: [["2026-03-01", 5], ["2026-03-02", 8], ["2026-03-03", 6], ["2026-03-04", 9]] }];
    render(ZoomHarness, { chart: LineTimeChart, series, range: ["2026-03-02", "2026-03-03"] });
    await tick();
    expect(drawn().dataZoom[1]).toMatchObject({ fillerColor: theme.accentSoft, xAxisIndex: 0 });
    expect(drawn().dataZoom[1].start).toBeCloseTo(100 / 3);
    expect(drawn().dataZoom[1].end).toBeCloseTo(200 / 3);
    expect(screen.getByRole("group").getAttribute("aria-label")).toMatch(/^Mar 2, 2026 – Mar 3, 2026: Visits: from 8 to 6/);
  });
});

describe("Chart, with a data zoom of its own", () => {
  it("has the window as the others: in the option, styled, and bound", async () => {
    const option = {
      xAxis: { type: "category", data: months },
      yAxis: { type: "value" },
      series: [{ type: "line", name: "Revenue", data: [100, 120, 150, 260, 200] }],
      dataZoom: [{ type: "inside" }, { type: "slider" }],
    };
    render(ZoomHarness, { chart: Chart, option, range: ["Feb", "Apr"] });
    await tick();
    expect(drawn().dataZoom[1]).toMatchObject({ start: 25, end: 75, xAxisIndex: 0, fillerColor: theme.accentSoft });
    chart().handlers.datazoom({ start: 50, end: 100 });
    await tick();
    expect(bound()).toEqual(["Mar", "May"]);
  });
});

describe("persist", () => {
  it("the window comes back from the settings store, and goes into it", async () => {
    saved.set("elyra-charts.zoom.sales", ["Feb", "Mar"]);
    render(ZoomHarness, { ...sales, persist: "sales" });
    await tick();
    await new Promise((r) => setTimeout(r, 0));
    await tick();
    expect(bound()).toEqual(["Feb", "Mar"]);
    expect(lastZoom()).toEqual({ type: "dataZoom", start: 25, end: 50 });
    chart().handlers.datazoom({ start: 75, end: 100 });
    await new Promise((r) => setTimeout(r, 350));
    expect(store.set).toHaveBeenLastCalledWith("elyra-charts.zoom.sales", ["Apr", "May"]);
  });
});
