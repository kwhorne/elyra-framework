// WireCharts' combination, scatter & bubble and pie-variation builders
// (Option.php comboItem … roseChart), ported as they are (RFC 0059).
import { or, truthy, isArray, values, merge, normalizeSeries, normalizePie } from "./helpers.js";

/** PHP's `round($value, $precision)`: halves away from zero. */
function round(value, precision = 0) {
  const m = 10 ** precision;
  const r = Math.round(Math.abs(value) * m) / m;
  return value < 0 ? -r : r;
}

/** PHP's `array_map` on one array: a list stays a list, a map keeps its keys. */
function map(value, fn) {
  if (Array.isArray(value)) return value.map((v) => fn(v));
  if (isArray(value)) return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, fn(v)]));
  return [];
}

/** PHP's `array_sum`: numeric values added, anything else counted as 0. */
const sum = (list) => list.reduce((total, v) => total + (Number(v) || 0), 0);

/** One series of a combination chart, its friendly `type` (bar, line, area) made ECharts'. */
function comboItem(s) {
  const type = or(s.type, "bar");
  const { type: _type, axis: _axis, ...rest } = s;

  const item = merge({ type: type === "area" ? "line" : type }, rest);

  if (type === "area") {
    item.areaStyle = {};
    item.smooth = true;
  }
  if (type === "line") {
    item.smooth = or(item.smooth, true);
    item.symbolSize = 7;
    item.lineStyle = { width: 3 };
    item.z = 3;
  }

  return item;
}

/** A combo series aimed at its `axis` (0 when unset). */
function axisItem(s) {
  const axis = or(s.axis, 0);
  const item = comboItem(s);
  item.yAxisIndex = axis;

  return item;
}

// ---- Combinations ---------------------------------------------------------

/** Combined line & column chart on a single axis. */
export function comboLineColumn(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, true);

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "cross" } },
    legend: { show: legend },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 40, containLabel: true },
    xAxis: { type: "category", data: categories },
    yAxis: { type: "value" },
    series: values(series).map((s) => comboItem(s)),
  };
}

/** Dual-axis combination: each series targets the left (0) or right (1) axis. */
export function comboDualAxis(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, true);
  const axisNames = or(args.axisNames, ["", ""]);

  const seriesOption = values(series).map(axisItem);

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "cross" } },
    legend: { show: legend },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 40, containLabel: true },
    xAxis: { type: "category", data: categories },
    yAxis: [
      { type: "value", name: or(axisNames[0], "") },
      { type: "value", name: or(axisNames[1], ""), splitLine: { show: false } },
    ],
    series: seriesOption,
  };
}

/** Multiple-axis combination: each series targets one of several y-axes. */
export function comboMultiAxis(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, true);
  const axes = or(args.axes, ["", "", ""]);

  const yAxis = values(axes).map((name, i) => ({
    type: "value",
    name,
    position: i === 0 ? "left" : "right",
    offset: i >= 2 ? (i - 1) * 55 : 0,
    splitLine: { show: i === 0 },
  }));

  const seriesOption = values(series).map(axisItem);

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "cross" } },
    legend: { show: legend },
    grid: { left: "5%", right: 70, bottom: "3%", top: 40, containLabel: true },
    xAxis: { type: "category", data: categories },
    yAxis,
    series: seriesOption,
  };
}

// ---- Scatter & bubble -----------------------------------------------------

/** Scatter plot with a least-squares linear regression (trend) line. */
export function scatterRegression(args) {
  const series = normalizeSeries(or(args.series, []));
  const points = or(series[0]?.data, []);
  const name = or(series[0]?.name, "Data");

  const list = values(points);
  const n = list.length;
  let sx = 0;
  let sy = 0;
  let sxy = 0;
  let sxx = 0;
  let xmin = Infinity;
  let xmax = -Infinity;
  for (const p of list) {
    const x = or(p?.[0], 0);
    const y = or(p?.[1], 0);
    sx += x;
    sy += y;
    sxy += x * y;
    sxx += x * x;
    xmin = Math.min(xmin, x);
    xmax = Math.max(xmax, x);
  }

  let line = [];
  const denom = n * sxx - sx * sx;
  if (n > 1 && denom !== 0) {
    const slope = (n * sxy - sx * sy) / denom;
    const intercept = (sy - slope * sx) / n;
    line = [
      [xmin, round(intercept + slope * xmin, 2)],
      [xmax, round(intercept + slope * xmax, 2)],
    ];
  }

  return {
    tooltip: { trigger: "item" },
    legend: { show: or(args.legend, true) },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 40, containLabel: true },
    xAxis: { type: "value", scale: true },
    yAxis: { type: "value", scale: true },
    series: [
      { name, type: "scatter", symbolSize: 10, data: points },
      { name: "Trend", type: "line", showSymbol: false, data: line, lineStyle: { width: 2, type: "dashed" } },
    ],
  };
}

/** Multi-series scatter where each series uses a distinct point symbol. */
export function scatterSymbols(args) {
  const series = normalizeSeries(or(args.series, []));
  const legend = or(args.legend, true);
  const symbols = ["circle", "rect", "triangle", "diamond", "pin", "arrow"];

  const seriesOption = values(series).map((s, i) =>
    merge({ type: "scatter", symbol: symbols[i % symbols.length], symbolSize: 12 }, s),
  );

  return {
    tooltip: { trigger: "item" },
    legend: { show: legend },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 40, containLabel: true },
    xAxis: { type: "value", scale: true },
    yAxis: { type: "value", scale: true },
    series: seriesOption,
  };
}

/**
 * Packed bubble chart: force-packed bubbles sized by value and grouped by
 * category, with no links.
 */
export function packedBubble(args) {
  const nodes = or(args.nodes, []);
  const categories = or(args.categories, []);

  let maxVal = 0;
  for (const node of values(nodes)) {
    maxVal = Math.max(maxVal, or(node.value, 0));
  }
  const scale = maxVal > 0 ? 60 / Math.sqrt(maxVal) : 1;

  const data = map(nodes, (node) => {
    const value = or(node.value, 0);
    const bubble = {
      name: or(node.name, ""),
      value,
      symbolSize: Math.max(12, Math.sqrt(value) * scale),
      category: or(node.category, null),
    };
    return Object.fromEntries(Object.entries(bubble).filter(([, v]) => v !== null));
  });

  return {
    tooltip: { trigger: "item", formatter: "{b}: {c}" },
    legend: { show: truthy(categories), data: categories },
    series: [
      {
        type: "graph",
        layout: "force",
        roam: true,
        draggable: true,
        data,
        categories: map(categories, (c) => ({ name: c })),
        force: { repulsion: 80, gravity: 0.2, edgeLength: 10 },
        label: { show: true, position: "inside", fontSize: 10 },
        emphasis: { focus: "self" },
      },
    ],
  };
}

// ---- Pie variations -------------------------------------------------------

/** Semi-circle donut: slices fill the top half of a ring. */
export function pieSemi(args) {
  const data = normalizePie(or(args.series, []), or(args.labels, []));
  const legend = or(args.legend, true);

  const total = sum(data.map((d) => (isArray(d) ? or(d.value, 0) : d)));
  data.push({
    value: total,
    name: "",
    itemStyle: { color: "transparent" },
    label: { show: false },
    tooltip: { show: false },
    emphasis: { disabled: true },
  });

  return {
    tooltip: { trigger: "item" },
    legend: { show: legend, bottom: 0 },
    series: [
      {
        type: "pie",
        radius: ["45%", "75%"],
        center: ["50%", "70%"],
        startAngle: 180,
        label: { show: true, formatter: "{b}" },
        data,
      },
    ],
  };
}

/** Pie with external data labels and leader lines. */
export function pieLabels(args) {
  const data = normalizePie(or(args.series, []), or(args.labels, []));

  return {
    tooltip: { trigger: "item" },
    legend: { show: false },
    series: [
      {
        type: "pie",
        radius: "55%",
        center: ["50%", "50%"],
        label: { show: true, position: "outside", formatter: "{b}: {d}%" },
        labelLine: { show: true, length: 16, length2: 12 },
        data,
      },
    ],
  };
}

/** Monochrome pie: slices in shades of a single hue. */
export function pieMonochrome(args) {
  const data = normalizePie(or(args.series, []), or(args.labels, []));
  const [r, g, b] = or(args.color, [249, 115, 22]);
  const legend = or(args.legend, true);
  const n = data.length;

  data.forEach((d, i) => {
    const t = n > 1 ? (i / (n - 1)) * 0.8 : 0;
    const rr = Math.trunc(round(r + t * (255 - r)));
    const gg = Math.trunc(round(g + t * (255 - g)));
    const bb = Math.trunc(round(b + t * (255 - b)));
    data[i] = merge(isArray(d) ? d : { value: d }, { itemStyle: { color: `rgb(${rr},${gg},${bb})` } });
  });

  return {
    tooltip: { trigger: "item" },
    legend: { show: legend, bottom: 0 },
    series: [
      {
        type: "pie",
        radius: "70%",
        data,
      },
    ],
  };
}

/** Gradient pie: each slice filled with a radial gradient. */
export function pieGradient(args) {
  const data = normalizePie(or(args.series, []), or(args.labels, []));
  const legend = or(args.legend, true);
  const donut = or(args.donut, true);
  const palette = [
    [249, 115, 22],
    [14, 165, 233],
    [34, 197, 94],
    [168, 85, 247],
    [239, 68, 68],
    [234, 179, 8],
  ];

  data.forEach((d, i) => {
    const [r, g, b] = palette[i % palette.length];
    data[i] = merge(isArray(d) ? d : { value: d }, {
      itemStyle: {
        color: {
          type: "radial",
          x: 0.5,
          y: 0.5,
          r: 0.75,
          colorStops: [
            { offset: 0, color: `rgba(${r},${g},${b},0.55)` },
            { offset: 1, color: `rgb(${r},${g},${b})` },
          ],
        },
      },
    });
  });

  return {
    tooltip: { trigger: "item" },
    legend: { show: legend, bottom: 0 },
    series: [
      {
        type: "pie",
        radius: truthy(donut) ? ["45%", "72%"] : "72%",
        itemStyle: { borderRadius: 6, borderColor: "transparent", borderWidth: 2 },
        data,
      },
    ],
  };
}

/** Variable radius pie (roseType 'radius'): each slice's radius encodes value. */
export function pieVariable(args) {
  return roseChart(args, "radius");
}

/** Nightingale rose (roseType 'area'): slice area encodes value. */
export function pieRose(args) {
  return roseChart(args, "area");
}

/** The rose pie both variations share, by `roseType`. */
function roseChart(args, roseType) {
  const data = normalizePie(or(args.series, []), or(args.labels, []));
  const legend = or(args.legend, true);

  return {
    tooltip: { trigger: "item" },
    legend: { show: legend, bottom: 0 },
    series: [
      {
        type: "pie",
        radius: ["20%", "75%"],
        roseType,
        itemStyle: { borderRadius: 5 },
        data,
      },
    ],
  };
}

// ---- Components -----------------------------------------------------------

/** Each component's call, as its Blade view makes it. */
/** WireCharts' `combo-line-column`, as its view calls its builder. */
export const callComboLineColumn = (p) =>
  comboLineColumn({ series: p.series, categories: p.categories, legend: p.legend });

/** WireCharts' `combo-dual-axis`, as its view calls its builder. */
export const callComboDualAxis = (p) =>
  comboDualAxis({ series: p.series, categories: p.categories, axisNames: p.axisNames, legend: p.legend });

/** WireCharts' `combo-multi-axis`, as its view calls its builder. */
export const callComboMultiAxis = (p) =>
  comboMultiAxis({ series: p.series, categories: p.categories, axes: p.axes, legend: p.legend });

/** WireCharts' `scatter-regression`, as its view calls its builder. */
export const callScatterRegression = (p) => scatterRegression({ series: p.series, legend: p.legend });

/** WireCharts' `scatter-symbols`, as its view calls its builder. */
export const callScatterSymbols = (p) => scatterSymbols({ series: p.series, legend: p.legend });

/** WireCharts' `packed-bubble`, as its view calls its builder. */
export const callPackedBubble = (p) => packedBubble({ nodes: p.nodes, categories: p.categories });

/** WireCharts' `pie-semi`, as its view calls its builder. */
export const callPieSemi = (p) => pieSemi({ series: p.series, labels: p.labels, legend: p.legend });

/** WireCharts' `pie-labels`, as its view calls its builder. */
export const callPieLabels = (p) => pieLabels({ series: p.series, labels: p.labels, legend: p.legend });

/** WireCharts' `pie-monochrome`, as its view calls its builder. */
export const callPieMonochrome = (p) => pieMonochrome({ series: p.series, labels: p.labels, legend: p.legend });

/** WireCharts' `pie-gradient`, as its view calls its builder. */
export const callPieGradient = (p) => pieGradient({ series: p.series, labels: p.labels, legend: p.legend });

/** WireCharts' `pie-variable`, as its view calls its builder. */
export const callPieVariable = (p) => pieVariable({ series: p.series, labels: p.labels, legend: p.legend });

/** WireCharts' `pie-rose`, as its view calls its builder. */
export const callPieRose = (p) => pieRose({ series: p.series, labels: p.labels, legend: p.legend });

/** Every call here, by WireCharts' name (the tests'; a component imports its own). */
export const CALLS = {
  "combo-line-column": callComboLineColumn,
  "combo-dual-axis": callComboDualAxis,
  "combo-multi-axis": callComboMultiAxis,
  "scatter-regression": callScatterRegression,
  "scatter-symbols": callScatterSymbols,
  "packed-bubble": callPackedBubble,
  "pie-semi": callPieSemi,
  "pie-labels": callPieLabels,
  "pie-monochrome": callPieMonochrome,
  "pie-gradient": callPieGradient,
  "pie-variable": callPieVariable,
  "pie-rose": callPieRose,
};
