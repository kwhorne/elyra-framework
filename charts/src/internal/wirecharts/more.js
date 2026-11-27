// The rest of WireCharts' charts (RFC 0059): scatter to the 3D ones, ported
// from Option.php's builders as they are, each called as its Blade view calls it.
import { or, truthy, isList, isArray, merge, normalizeSeries, normalizePie } from "./helpers.js";

/** PHP's `array_replace_recursive($base, $replacements)`: arrays merged key by key, anything else replaced. */
function replaceRecursive(base, replacements) {
  if (!isArray(replacements)) return base;
  const keys = Object.keys(replacements);
  const asList = isList(base) && keys.every((k) => /^(0|[1-9]\d*)$/.test(k));
  const out = asList ? [...base] : { ...base };
  for (const key of keys) {
    const next = replacements[key];
    const current = out[key];
    out[key] = isArray(current) && isArray(next) ? replaceRecursive(current, next) : next;
  }
  return out;
}

/** Scatter, or bubble when `bubble` sizes each point by its third value. */
export function scatter(args, bubble = false) {
  const series = normalizeSeries(or(args.series, []));
  const legend = or(args.legend, true);

  const seriesOption = series.map((s) => {
    const item = merge({ type: "scatter" }, s);
    if (bubble) {
      item.symbolSize = function (d) {
        return Math.sqrt(d[2] || 1) * 4;
      };
    }
    return item;
  });

  return {
    tooltip: { trigger: "item" },
    legend: { show: legend },
    grid: { left: "3%", right: "4%", bottom: "3%", top: truthy(legend) ? 40 : 16, containLabel: true },
    xAxis: { type: "value", scale: true },
    yAxis: { type: "value", scale: true },
    series: seriesOption,
  };
}

/** Pie / donut. Data is `[{ name, value }, …]` or values named by `labels`. */
export function pie(args, donut = false) {
  const data = normalizePie(or(args.series, []), or(args.labels, []));
  const legend = or(args.legend, true);
  const roseType = or(args.rose, false);

  return {
    tooltip: { trigger: "item" },
    legend: { show: legend, bottom: 0 },
    series: [
      {
        type: "pie",
        radius: donut ? ["45%", "70%"] : truthy(roseType) ? ["20%", "70%"] : "70%",
        roseType: truthy(roseType) ? "area" : false,
        avoidLabelOverlap: true,
        itemStyle: { borderRadius: donut ? 6 : 0 },
        data,
      },
    ],
  };
}

/** Gauge. value 0..max, with a needle or a progress ring. */
export function gauge(args) {
  const value = or(args.value, 0);
  const max = or(args.max, 100);
  const min = or(args.min, 0);
  const name = or(args.name, "");
  const progress = or(args.progress, false);

  return {
    tooltip: { formatter: "{a} <br/>{b} : {c}" },
    series: [
      {
        type: "gauge",
        min,
        max,
        progress: { show: truthy(progress), width: 12 },
        pointer: { show: !truthy(progress) },
        axisLine: { lineStyle: { width: truthy(progress) ? 12 : 18 } },
        data: [{ value, name }],
      },
    ],
  };
}

/**
 * An analog clock built from three gauge series (hour, minute, second).
 * The hands are advanced client-side every second.
 */
export function clock(args) {
  const accent = or(args.color, "#f97316");
  const hand = "#71717a";

  const hideZero = function (value) {
    return value === 0 ? "" : value;
  };

  const base = {
    animationDurationUpdate: 300,
    animationEasingUpdate: "linear",
    series: [
      {
        name: "hour",
        type: "gauge",
        startAngle: 90,
        endAngle: -270,
        min: 0,
        max: 12,
        splitNumber: 12,
        clockwise: true,
        axisLine: { lineStyle: { width: 6, color: [[1, "rgba(127,127,127,0.25)"]] } },
        splitLine: { lineStyle: { width: 3, color: "#9ca3af" } },
        axisTick: { splitNumber: 5, lineStyle: { width: 1, color: "#c0c4cc" } },
        axisLabel: { distance: 20, fontSize: 14, formatter: hideZero },
        pointer: { width: 8, length: "52%", itemStyle: { color: hand } },
        anchor: { show: true, size: 16, itemStyle: { color: hand } },
        detail: { show: false },
        title: { show: false },
        data: [{ value: 0 }],
      },
      {
        name: "minute",
        type: "gauge",
        startAngle: 90,
        endAngle: -270,
        min: 0,
        max: 60,
        clockwise: true,
        axisLine: { show: false },
        splitLine: { show: false },
        axisTick: { show: false },
        axisLabel: { show: false },
        pointer: { width: 5, length: "72%", itemStyle: { color: hand } },
        anchor: { show: true, size: 12, itemStyle: { color: hand } },
        detail: { show: false },
        title: { show: false },
        data: [{ value: 0 }],
      },
      {
        name: "second",
        type: "gauge",
        startAngle: 90,
        endAngle: -270,
        min: 0,
        max: 60,
        clockwise: true,
        axisLine: { show: false },
        splitLine: { show: false },
        axisTick: { show: false },
        axisLabel: { show: false },
        pointer: { width: 2, length: "85%", itemStyle: { color: accent } },
        anchor: { show: true, size: 7, showAbove: true, itemStyle: { color: accent } },
        detail: { show: false },
        title: { show: false },
        data: [{ value: 0 }],
      },
    ],
  };

  return replaceRecursive(base, or(args.options, []));
}

/** Radar: one polygon per series over the `indicators`. */
export function radar(args) {
  const indicators = or(args.indicators, []);
  const series = normalizeSeries(or(args.series, []));
  const legend = or(args.legend, true);

  return {
    tooltip: { trigger: "item" },
    legend: { show: legend, bottom: 0 },
    radar: { indicator: indicators },
    series: [
      {
        type: "radar",
        data: series.map((s) => ({
          name: or(s.name, ""),
          value: or(s.data, or(s.value, [])),
        })),
      },
    ],
  };
}

/** Funnel: pie-shaped data as stages, labelled inside. */
export function funnel(args) {
  return {
    tooltip: { trigger: "item", formatter: "{b} : {c}" },
    legend: { show: or(args.legend, true), bottom: 0 },
    series: [
      {
        type: "funnel",
        left: "10%",
        width: "80%",
        label: { show: true, position: "inside" },
        data: normalizePie(or(args.series, []), or(args.labels, [])),
      },
    ],
  };
}

/** Heatmap: `[x, y, value]` cells over category axes, with a visual map up to `max`. */
export function heatmap(args) {
  const x = or(args.categories, []);
  const y = or(args.rows, []);
  const data = or(args.series, []);
  const max = or(args.max, 10);

  return {
    tooltip: { position: "top" },
    grid: { height: "70%", top: "5%", containLabel: true },
    xAxis: { type: "category", data: x, splitArea: { show: true } },
    yAxis: { type: "category", data: y, splitArea: { show: true } },
    visualMap: { min: 0, max, calculable: true, orient: "horizontal", left: "center", bottom: 0 },
    series: [
      {
        type: "heatmap",
        data,
        label: { show: false },
        emphasis: { itemStyle: { shadowBlur: 10 } },
      },
    ],
  };
}

/** Candlestick: `[open, close, low, high]` per category. */
export function candlestick(args) {
  return {
    tooltip: { trigger: "axis", axisPointer: { type: "cross" } },
    grid: { left: "3%", right: "4%", bottom: "3%", containLabel: true },
    xAxis: { type: "category", data: or(args.categories, []) },
    yAxis: { type: "value", scale: true },
    series: [{ type: "candlestick", data: or(args.series, []) }],
  };
}

/** Boxplot: `[min, Q1, median, Q3, max]` per category. */
export function boxplot(args) {
  return {
    tooltip: { trigger: "item" },
    grid: { left: "3%", right: "4%", bottom: "3%", containLabel: true },
    xAxis: { type: "category", data: or(args.categories, []) },
    yAxis: { type: "value" },
    series: [{ type: "boxplot", data: or(args.series, []) }],
  };
}

/** Tree: one root node and its children, expandable. */
export function tree(args) {
  return {
    tooltip: { trigger: "item", triggerOn: "mousemove" },
    series: [
      {
        type: "tree",
        data: [or(args.data, [])],
        top: "2%",
        left: "10%",
        bottom: "2%",
        right: "20%",
        symbolSize: 8,
        expandAndCollapse: true,
        label: { position: "left", align: "right" },
        leaves: { label: { position: "right", align: "left" } },
      },
    ],
  };
}

/** Treemap: nested `{ name, value, children }` as nested rectangles. */
export function treemap(args) {
  return {
    tooltip: { trigger: "item" },
    series: [
      {
        type: "treemap",
        data: or(args.data, or(args.series, [])),
        roam: false,
        label: { show: true },
      },
    ],
  };
}

/** Sunburst: nested `{ name, value, children }` as rings. */
export function sunburst(args) {
  return {
    tooltip: { trigger: "item" },
    series: [
      {
        type: "sunburst",
        data: or(args.data, or(args.series, [])),
        radius: [0, "90%"],
        label: { rotate: "radial" },
      },
    ],
  };
}

/** Sankey: flows between nodes, `links` of `{ source, target, value }`. */
export function sankey(args) {
  return {
    tooltip: { trigger: "item", triggerOn: "mousemove" },
    series: [
      {
        type: "sankey",
        data: or(args.nodes, []),
        links: or(args.links, []),
        emphasis: { focus: "adjacency" },
        lineStyle: { color: "gradient", curveness: 0.5 },
      },
    ],
  };
}

/** Graph: a network of nodes and links, force-laid out by default. */
export function graph(args) {
  return {
    tooltip: [],
    legend: { show: or(args.legend, false) },
    series: [
      {
        type: "graph",
        layout: or(args.layout, "force"),
        roam: true,
        draggable: true,
        data: or(args.nodes, []),
        links: or(args.links, []),
        categories: or(args.categories, []),
        force: { repulsion: 120, edgeLength: 80 },
        label: { show: true, position: "right" },
        emphasis: { focus: "adjacency" },
      },
    ],
  };
}

/** 3D bars on a category grid (requires the GL extension). */
export function bar3d(args) {
  return grid3d("bar3D", args);
}

/** 3D scatter (requires the GL extension). */
export function scatter3d(args) {
  return grid3d("scatter3D", args);
}

/** 3D surface from `[x, y, z]` points (requires the GL extension). */
export function surface(args) {
  return {
    tooltip: [],
    xAxis3D: { type: "value" },
    yAxis3D: { type: "value" },
    zAxis3D: { type: "value" },
    grid3D: { viewControl: { autoRotate: or(args.rotate, false) } },
    series: [
      {
        type: "surface",
        data: or(args.series, or(args.data, [])),
      },
    ],
  };
}

/** The 3D grid bar3d and scatter3d share: axes, a rotating view, one series of `type`. */
function grid3d(type, args) {
  const xAxis = { type: or(args.xType, "category") };
  if (or(args.categories, null) !== null) xAxis.data = args.categories;
  const yAxis = { type: or(args.yType, "category") };
  if (or(args.rows, null) !== null) yAxis.data = args.rows;

  const option = {
    tooltip: [],
    xAxis3D: xAxis,
    yAxis3D: yAxis,
    zAxis3D: { type: "value" },
    grid3D: { viewControl: { autoRotate: or(args.rotate, false) } },
    series: [
      {
        type,
        data: or(args.series, or(args.data, [])),
        shading: "lambert",
      },
    ],
  };

  if (or(args.visualMap, null) !== null) option.visualMap = args.visualMap;

  return option;
}

/** PHP's `array_filter($map, fn ($v) => $v !== null)`. */
const withoutNulls = (map) => Object.fromEntries(Object.entries(map).filter(([, v]) => v !== null));

/** WireCharts' `scatter`, as its view calls its builder. */
export const callScatter = (props) => scatter({ series: props.series, legend: props.legend }, false);

/** WireCharts' `bubble`, as its view calls its builder. */
export const callBubble = (props) => scatter({ series: props.series, legend: props.legend }, true);

/** WireCharts' `pie`, as its view calls its builder. */
export const callPie = (props) =>
  pie({ series: props.series, labels: props.labels, legend: props.legend, rose: props.rose }, false);

/** WireCharts' `donut`, as its view calls its builder. */
export const callDonut = (props) => pie({ series: props.series, labels: props.labels, legend: props.legend }, true);

/** WireCharts' `gauge`, as its view calls its builder. */
export const callGauge = (props) =>
  gauge({ value: props.value, min: props.min, max: props.max, name: props.name, progress: props.progress });

/** WireCharts' `clock`, as its view calls its builder. */
export const callClock = (props) => clock({ color: props.color, options: props.options });

/** WireCharts' `radar`, as its view calls its builder. */
export const callRadar = (props) => radar({ series: props.series, indicators: props.indicators, legend: props.legend });

/** WireCharts' `funnel`, as its view calls its builder. */
export const callFunnel = (props) => funnel({ series: props.series, labels: props.labels, legend: props.legend });

/** WireCharts' `heatmap`, as its view calls its builder. */
export const callHeatmap = (props) =>
  heatmap({ series: props.series, categories: props.categories, rows: props.rows, max: props.max });

/** WireCharts' `candlestick`, as its view calls its builder. */
export const callCandlestick = (props) => candlestick({ series: props.series, categories: props.categories });

/** WireCharts' `boxplot`, as its view calls its builder. */
export const callBoxplot = (props) => boxplot({ series: props.series, categories: props.categories });

/** WireCharts' `tree`, as its view calls its builder. */
export const callTree = (props) => tree({ data: props.data });

/** WireCharts' `treemap`, as its view calls its builder. */
export const callTreemap = (props) => treemap({ data: props.data, series: props.series });

/** WireCharts' `sunburst`, as its view calls its builder. */
export const callSunburst = (props) => sunburst({ data: props.data, series: props.series });

/** WireCharts' `sankey`, as its view calls its builder. */
export const callSankey = (props) => sankey({ nodes: props.nodes, links: props.links });

/** WireCharts' `graph`, as its view calls its builder. */
export const callGraph = (props) =>
  graph({
    nodes: props.nodes,
    links: props.links,
    categories: props.categories,
    layout: props.layout,
    legend: props.legend,
  });

/** WireCharts' `bar3d`, as its view calls its builder. */
export const callBar3d = (props) =>
  bar3d(
    withoutNulls({
      series: props.series,
      categories: props.categories,
      rows: props.rows,
      rotate: props.rotate,
      visualMap: truthy(props.max) ? { max: props.max, calculable: true, right: 0, top: "center" } : null,
    }),
  );

/** WireCharts' `scatter3d`, as its view calls its builder. */
export const callScatter3d = (props) =>
  scatter3d({ series: props.series, xType: props.xType, yType: props.yType, rotate: props.rotate });

/** WireCharts' `surface`, as its view calls its builder. */
export const callSurface = (props) => surface({ series: props.series, data: props.data, rotate: props.rotate });

/** Every call here, by WireCharts' name (the tests'; a component imports its own). */
export const CALLS = {
  "scatter": callScatter,
  "bubble": callBubble,
  "pie": callPie,
  "donut": callDonut,
  "gauge": callGauge,
  "clock": callClock,
  "radar": callRadar,
  "funnel": callFunnel,
  "heatmap": callHeatmap,
  "candlestick": callCandlestick,
  "boxplot": callBoxplot,
  "tree": callTree,
  "treemap": callTreemap,
  "sunburst": callSunburst,
  "sankey": callSankey,
  "graph": callGraph,
  "bar3d": callBar3d,
  "scatter3d": callScatter3d,
  "surface": callSurface,
};
