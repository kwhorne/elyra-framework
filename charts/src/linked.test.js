// Linked charts and a selection (RFC 0063): a window and a pointer shared by
// value; a brush whose selection is the app's data; the keys; what it says.
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/svelte";
import { tick } from "svelte";
import { charts } from "./test/echarts.js";
import SelectHarness from "./test/SelectHarness.svelte";
import { themeOf } from "./internal/theme.js";
import { withAlpha } from "./internal/brush.js";
import { LineChart, ScatterChart } from "./index.js";

const theme = themeOf(null);
const months = ["Jan", "Feb", "Mar", "Apr", "May"];
const sales = { series: [{ name: "Revenue", data: [100, 120, 150, 260, 200] }], categories: months };
const drawn = (c) => c.options.at(-1).option;
const actions = (c, type) => c.actions.filter((a) => a.type === type);
const bound = () => JSON.parse(document.querySelector("output").textContent);
const key = (el, k) => el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));

describe("link", () => {
  it("zoom one, and the others show the same values, each on its own axis", async () => {
    render(LineChart, { ...sales, zoom: true, link: "sales" });
    render(LineChart, { series: [{ name: "Orders", data: [3, 4, 5] }], categories: ["Feb", "Mar", "Apr"], zoom: true, link: "sales" });
    await tick();
    const [a, b] = charts;
    a.handlers.datazoom({ start: 25, end: 50 });
    await tick();
    // Feb – Mar on b's axis is its first two of three.
    expect(actions(b, "dataZoom").at(-1)).toEqual({ type: "dataZoom", start: 0, end: 50 });
    // b's echo doesn't move a again.
    b.handlers.datazoom({ start: 0, end: 50 });
    expect(actions(a, "dataZoom")).toHaveLength(0);
  });

  it("a value one chart doesn't have is the nearest it has", async () => {
    render(LineChart, { series: [{ data: [1, 2, 3, 4] }], categories: ["2026-03-01", "2026-03-08", "2026-03-15", "2026-03-22"], zoom: true, link: "weeks" });
    await tick();
    render(LineChart, { series: [{ data: [1, 2, 3, 4, 5, 6, 7, 8] }], categories: ["2026-03-01", "2026-03-04", "2026-03-07", "2026-03-10", "2026-03-13", "2026-03-16", "2026-03-19", "2026-03-22"], zoom: true, link: "weeks" });
    await tick();
    const [weeks, days] = charts;
    days.handlers.datazoom({ start: 0, end: 50 });
    await tick();
    // Days 1 – 10 March, on the weeks' axis by time: 10 March is 9 of its 21 days.
    const moved = actions(weeks, "dataZoom").at(-1);
    expect(moved.start).toBeCloseTo(0);
    expect(moved.end).toBeCloseTo((9 / 21) * 100);
  });

  it("the pointer: over one, each shows its tooltip at the same value; out, they hide", async () => {
    render(LineChart, { ...sales, link: "hover" });
    render(LineChart, { series: [{ name: "Orders", data: [3, 4, 5] }], categories: ["Feb", "Mar", "Apr"], link: "hover" });
    await tick();
    const [a, b] = charts;
    a.handlers.updateAxisPointer({ axesInfo: [{ axisDim: "x", axisIndex: 0, value: 2 }] });
    expect(actions(b, "showTip").at(-1)).toEqual({ type: "showTip", seriesIndex: 0, dataIndex: 1 });
    // b's own pointer moving to it isn't passed back.
    b.handlers.updateAxisPointer({ axesInfo: [{ axisDim: "x", axisIndex: 0, value: 1 }] });
    expect(actions(a, "showTip")).toHaveLength(0);
    a.handlers.globalout();
    expect(actions(b, "hideTip")).toHaveLength(1);
  });

  it("says it's linked, and with how many", async () => {
    render(LineChart, { ...sales, link: "say", label: "Revenue" });
    const second = render(LineChart, { ...sales, link: "say", label: "Orders" });
    render(LineChart, { ...sales, link: "say", label: "Returns" });
    await tick();
    expect(screen.getByRole("group", { name: "Revenue" }).getAttribute("aria-description")).toBe("Linked with 2 other charts");
    second.unmount();
    await tick();
    expect(screen.getByRole("group", { name: "Revenue" }).getAttribute("aria-description")).toBe("Linked with one other chart");
  });
});

describe("brush", () => {
  const points = { series: [{ name: "Customers", data: [[1, 10], [2, 20], [3, 30]] }] };

  it("a scatter: a rectangle in the tokens, and what's under it as its points", async () => {
    render(SelectHarness, { chart: ScatterChart, ...points });
    await tick();
    const [c] = charts;
    expect(drawn(c).brush).toMatchObject({
      toolbox: [],
      xAxisIndex: 0,
      yAxisIndex: 0,
      brushStyle: { borderColor: theme.accent, color: withAlpha(theme.accent, 0.15) },
    });
    expect(withAlpha("#7aa2f7", 0.15)).toBe("rgba(122, 162, 247, 0.15)");
    expect(withAlpha("#abc", 0.5)).toBe("rgba(170, 187, 204, 0.5)");
    expect(withAlpha("rgb(1 2 3)", 0.2)).toBe("rgba(1, 2, 3, 0.2)");
    expect(actions(c, "takeGlobalCursor").at(-1).brushOption).toEqual({ brushType: "rect", brushMode: "single" });
    c.handlers.brushSelected({ batch: [{ selected: [{ seriesIndex: 0, dataIndex: [0, 2] }] }] });
    await tick();
    expect(bound()).toEqual({
      points: [
        { series: "Customers", x: 1, y: 10, seriesIndex: 0, index: 0 },
        { series: "Customers", x: 3, y: 30, seriesIndex: 0, index: 2 },
      ],
    });
    const group = screen.getByRole("group");
    expect(group.getAttribute("aria-label")).toMatch(/2 of 3 selected$/);
    expect([...document.querySelectorAll("tbody th")].map((th) => th.textContent)).toEqual([
      "Customers, selected",
      "Customers",
      "Customers, selected",
    ]);
    c.handlers.brushSelected({ batch: [{ selected: [{ seriesIndex: 0, dataIndex: [] }] }] });
    await tick();
    expect(bound()).toBeNull();
  });

  it("along the categories: a span, and the app's own drawn", async () => {
    const { component } = render(SelectHarness, sales);
    await tick();
    const [c] = charts;
    expect(actions(c, "takeGlobalCursor").at(-1).brushOption.brushType).toBe("lineX");
    c.handlers.brushSelected({ batch: [{ selected: [{ seriesIndex: 0, dataIndex: [3, 1, 2] }] }] });
    await tick();
    expect(bound()).toEqual({ from: "Feb", to: "Apr", categories: ["Feb", "Mar", "Apr"] });
    expect(screen.getByRole("group").getAttribute("aria-label")).toMatch(/Feb – Apr selected \(3\)$/);
    component.set({ from: "Mar", to: "May", categories: ["Mar", "Apr", "May"] });
    await tick();
    expect(actions(c, "brush").at(-1)).toEqual({ type: "brush", areas: [{ brushType: "lineX", xAxisIndex: 0, coordRange: [2, 4] }] });
    component.set(null);
    await tick();
    expect(actions(c, "brush").at(-1)).toEqual({ type: "brush", areas: [] });
  });

  it("a drag brushes and the wheel zooms: the inside zoom doesn't pan", async () => {
    render(SelectHarness, { ...sales, zoom: true });
    await tick();
    expect(drawn(charts[0]).dataZoom[0]).toMatchObject({ type: "inside", moveOnMouseMove: false });
  });
});

describe("the keys", () => {
  it("Space starts a span, the arrows grow it, Space ends it, Esc clears it, each said", async () => {
    render(SelectHarness, sales);
    await tick();
    const group = screen.getByRole("group");
    // The harness's <output> is a status too: the chart's own is the live region.
    const status = () => document.querySelector('[aria-live="polite"]').textContent;
    key(group, "ArrowRight");
    key(group, " ");
    key(group, "ArrowRight");
    key(group, "ArrowRight");
    await tick();
    expect(bound()).toEqual({ from: "Jan", to: "Mar", categories: ["Jan", "Feb", "Mar"] });
    expect(status()).toBe("Jan – Mar selected (3)");
    expect(actions(charts[0], "brush").at(-1)).toEqual({ type: "brush", areas: [{ brushType: "lineX", xAxisIndex: 0, coordRange: [0, 2] }] });
    key(group, " ");
    key(group, "ArrowRight");
    await tick();
    expect(bound().to).toBe("Mar");
    key(group, "Escape");
    await tick();
    expect(bound()).toBeNull();
    expect(status()).toBe("Selection cleared");
  });

  it("over a scatter, Space takes the point in, and out again", async () => {
    render(SelectHarness, { chart: ScatterChart, series: [{ name: "Customers", data: [[1, 10], [2, 20]] }] });
    await tick();
    const group = screen.getByRole("group");
    key(group, "ArrowRight");
    key(group, "ArrowRight");
    key(group, " ");
    await tick();
    expect(bound()).toEqual({ points: [{ series: "Customers", x: 2, y: 20, seriesIndex: 0, index: 1 }] });
    expect(actions(charts[0], "select").at(-1)).toEqual({ type: "select", seriesIndex: 0, dataIndex: 1 });
    expect(document.querySelector('[aria-live="polite"]').textContent).toBe("1 of 2 selected");
    key(group, " ");
    await tick();
    expect(bound()).toBeNull();
    expect(actions(charts[0], "unselect").at(-1)).toEqual({ type: "unselect", seriesIndex: 0, dataIndex: 1 });
  });
});
