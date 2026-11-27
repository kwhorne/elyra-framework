// WireCharts' line-family builders, ported from Option.php (RFC 0059): each
// returns the option Option.php returns for the same arguments.
import { or, truthy, values, merge, normalizeSeries } from "./helpers.js";

const isString = (value) => typeof value === "string";

/** The grid most builders share: room for the legend when it shows. */
const grid = (legend, bottom = "3%") => ({
  left: "3%",
  right: "4%",
  bottom,
  top: truthy(legend) ? 40 : 16,
  containLabel: true,
});

/** Inside and slider zoom, which the zoomable builders add. */
const dataZoom = () => [{ type: "inside" }, { type: "slider" }];

/** Names a line at its end, for the end-labelled builders. */
const seriesName = function (params) {
  return params.seriesName;
};

// Cartesian charts: line, area, column (vertical bar), bar (horizontal).
export function cartesian(type, args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const stack = or(args.stack, false);
  const smooth = or(args.smooth, false);
  const step = or(args.step, false);
  const horizontal = or(args.horizontal, false);
  const legend = or(args.legend, true);

  const isArea = type === "area";
  const seriesType = isArea ? "line" : type;

  const seriesOption = series.map((s) => {
    const item = merge({ type: seriesType }, s);
    if (isArea) item.areaStyle = {};
    if (truthy(smooth)) item.smooth = true;
    if (step !== false) item.step = isString(step) ? step : "start";
    if (stack !== false) item.stack = isString(stack) ? stack : "total";
    return item;
  });

  const catAxis = {
    type: "category",
    data: categories,
    boundaryGap: type === "column" || type === "bar",
  };
  const valAxis = { type: "value" };

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: truthy(horizontal) ? valAxis : catAxis,
    yAxis: truthy(horizontal) ? catAxis : valAxis,
    series: seriesOption,
  };
}

// Spline with symbols: a smooth line where each series is marked with a
// distinct point symbol (circle, square, triangle, diamond, ...).
export function spline(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, true);
  const size = or(args.symbolSize, 8);
  const symbols = ["circle", "rect", "triangle", "diamond", "roundRect", "pin", "arrow"];

  const seriesOption = values(series).map((s, i) =>
    merge(
      {
        type: "line",
        smooth: true,
        symbol: symbols[i % symbols.length],
        symbolSize: size,
        showSymbol: true,
        lineStyle: { width: 3 },
      },
      s,
    ),
  );

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: { type: "category", data: categories, boundaryGap: false },
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

// Spline with inverted axes: a smooth line with the category axis running
// vertically and the value axis horizontally, for profiles such as
// temperature by altitude.
export function splineInverted(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, true);
  const size = or(args.symbolSize, 6);

  const seriesOption = values(series).map((s) =>
    merge(
      {
        type: "line",
        smooth: true,
        showSymbol: true,
        symbolSize: size,
        lineStyle: { width: 3 },
      },
      s,
    ),
  );

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: { type: "value" },
    yAxis: { type: "category", data: categories, boundaryGap: false },
    series: seriesOption,
  };
}

// Line with end labels: each series is named at the end of its line instead
// of in a legend, for comparing several labelled trends.
export function lineLabels(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, false);

  const seriesOption = values(series).map((s) =>
    merge(
      {
        type: "line",
        smooth,
        showSymbol: false,
        lineStyle: { width: 3 },
        emphasis: { focus: "series" },
        endLabel: { show: true, formatter: seriesName, fontWeight: "bold" },
      },
      s,
    ),
  );

  return {
    tooltip: { trigger: "axis" },
    legend: { show: false },
    grid: { left: "3%", right: 90, bottom: "3%", top: 16, containLabel: true },
    xAxis: { type: "category", data: categories, boundaryGap: false },
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

// Line with a logarithmic y-axis, for values spanning several orders of
// magnitude.
export function lineLog(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, false);
  const legend = or(args.legend, true);
  const logBase = or(args.logBase, 10);

  const seriesOption = values(series).map((s) =>
    merge(
      {
        type: "line",
        smooth,
        showSymbol: true,
        symbolSize: 6,
        lineStyle: { width: 3 },
      },
      s,
    ),
  );

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: { type: "category", data: categories, boundaryGap: false },
    yAxis: { type: "log", logBase },
    series: seriesOption,
  };
}

// Line race: a multi-series line whose points are revealed progressively
// (animated client-side), each line labelled at its leading end.
export function lineRace(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);

  const seriesOption = values(series).map((s) =>
    merge(
      {
        type: "line",
        smooth,
        showSymbol: false,
        lineStyle: { width: 3 },
        emphasis: { focus: "series" },
        endLabel: { show: true, formatter: seriesName, fontWeight: "bold" },
      },
      s,
    ),
  );

  return {
    tooltip: { trigger: "axis" },
    legend: { show: false },
    grid: { left: "3%", right: 90, bottom: "3%", top: 16, containLabel: true },
    xAxis: { type: "category", data: categories, boundaryGap: false },
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

// Line with a custom entrance animation: the line draws in with a chosen
// easing and a staggered per-point delay.
export function lineAnimated(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);
  const legend = or(args.legend, true);
  const duration = or(args.duration, 1200);
  const easing = or(args.easing, "cubicOut");

  const delay = function (idx) {
    return idx * 60;
  };

  const seriesOption = values(series).map((s) =>
    merge(
      {
        type: "line",
        smooth,
        showSymbol: true,
        symbolSize: 7,
        lineStyle: { width: 3 },
        animationDuration: duration,
        animationEasing: easing,
        animationDelay: delay,
      },
      s,
    ),
  );

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: { type: "category", data: categories, boundaryGap: false },
    yAxis: { type: "value" },
    animationDuration: duration,
    animationEasing: easing,
    series: seriesOption,
  };
}

// Line with a forecast: actual values drawn solid, forecast values continuing
// as a dashed line over a lightly shaded region.
export function lineForecast(args) {
  const categories = or(args.categories, []);
  const actual = values(or(args.actual, []));
  const forecast = values(or(args.forecast, []));
  const name = or(args.name, "Actual");
  const forecastName = or(args.forecastName, "Forecast");
  const smooth = or(args.smooth, true);

  const actualCount = actual.length;
  const nulls = (n) => new Array(n).fill(null);

  const actualData = [...actual, ...nulls(forecast.length)];
  const forecastData = [
    ...nulls(Math.max(0, actualCount - 1)),
    ...(actualCount ? [actual[actualCount - 1]] : []),
    ...forecast,
  ];

  const forecastSeries = {
    name: forecastName,
    type: "line",
    smooth,
    showSymbol: false,
    data: forecastData,
    lineStyle: { width: 3, type: "dashed" },
  };

  if (actualCount > 0 && truthy(categories)) {
    const count = values(categories).length;
    forecastSeries.markArea = {
      silent: true,
      itemStyle: { color: "rgba(127,127,127,0.08)" },
      data: [[{ xAxis: or(categories[actualCount - 1], null) }, { xAxis: or(categories[count - 1], null) }]],
    };
  }

  return {
    tooltip: { trigger: "axis" },
    legend: { show: true },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 40, containLabel: true },
    xAxis: { type: "category", data: categories, boundaryGap: false },
    yAxis: { type: "value" },
    series: [
      {
        name,
        type: "line",
        smooth,
        showSymbol: false,
        data: actualData,
        lineStyle: { width: 3 },
      },
      forecastSeries,
    ],
  };
}

// Line with annotations: labelled marker pins on chosen points, plus an
// optional average reference line.
export function lineAnnotated(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);
  const legend = or(args.legend, true);
  const annotations = or(args.annotations, []);
  const average = or(args.average, false);

  const markPointData = values(annotations).map((a) => ({
    coord: [or(a.x, null), or(a.y, null)],
    value: or(a.text, ""),
  }));

  const seriesOption = values(series).map((s, i) => {
    const item = merge(
      {
        type: "line",
        smooth,
        showSymbol: true,
        symbolSize: 6,
        lineStyle: { width: 3 },
      },
      s,
    );

    if (i === 0 && markPointData.length) {
      item.markPoint = {
        symbol: "pin",
        symbolSize: 48,
        label: { fontSize: 11, color: "#fff" },
        data: markPointData,
      };
    }

    if (i === 0 && truthy(average)) {
      item.markLine = { data: [{ type: "average", name: "Average" }] };
    }

    return item;
  });

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: { type: "category", data: categories, boundaryGap: false },
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

// Line tuned for very large datasets: LTTB downsampling, large-mode
// rendering, animation off, and zoom/pan controls.
export function lineBoost(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const sampling = or(args.sampling, "lttb");
  const zoom = or(args.zoom, true);
  const legend = or(args.legend, true);

  const seriesOption = values(series).map((s) =>
    merge(
      {
        type: "line",
        showSymbol: false,
        sampling,
        large: true,
        largeThreshold: 2000,
        lineStyle: { width: 1.5 },
      },
      s,
    ),
  );

  const option = {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    animation: false,
    grid: grid(legend, truthy(zoom) ? 60 : "3%"),
    xAxis: truthy(categories)
      ? { type: "category", data: categories, boundaryGap: false }
      : { type: "value", boundaryGap: false },
    yAxis: { type: "value" },
    series: seriesOption,
  };

  if (truthy(zoom)) option.dataZoom = dataZoom();

  return option;
}

// Time series: a line over a datetime axis, data points as [date, value]
// pairs, with optional zoom/pan.
export function lineTime(args) {
  const series = normalizeSeries(or(args.series, []));
  const smooth = or(args.smooth, false);
  const zoom = or(args.zoom, true);
  const legend = or(args.legend, true);

  const seriesOption = values(series).map((s) =>
    merge(
      {
        type: "line",
        smooth,
        showSymbol: false,
        lineStyle: { width: 2.5 },
      },
      s,
    ),
  );

  const option = {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend, truthy(zoom) ? 60 : "3%"),
    xAxis: { type: "time" },
    yAxis: { type: "value" },
    series: seriesOption,
  };

  if (truthy(zoom)) option.dataZoom = dataZoom();

  return option;
}

// Spline over a datetime axis with irregular intervals: points placed by
// their actual time and marked with symbols.
export function splineTime(args) {
  const series = normalizeSeries(or(args.series, []));
  const zoom = or(args.zoom, true);
  const legend = or(args.legend, true);
  const size = or(args.symbolSize, 6);

  const seriesOption = values(series).map((s) =>
    merge(
      {
        type: "line",
        smooth: true,
        showSymbol: true,
        symbolSize: size,
        lineStyle: { width: 3 },
      },
      s,
    ),
  );

  const option = {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend, truthy(zoom) ? 60 : "3%"),
    xAxis: { type: "time" },
    yAxis: { type: "value" },
    series: seriesOption,
  };

  if (truthy(zoom)) option.dataZoom = dataZoom();

  return option;
}

// Spline with horizontal plot bands: coloured background zones across the
// y-axis (value ranges), each optionally labelled.
export function splineBands(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);
  const legend = or(args.legend, true);
  const bands = or(args.bands, []);

  const bandData = values(bands).map((b) => {
    const start = { yAxis: or(b.from, 0) };

    if (truthy(b.color)) start.itemStyle = { color: b.color };
    if (truthy(b.label)) {
      start.label = {
        show: true,
        formatter: b.label,
        position: "insideStartTop",
        color: "#888",
        fontSize: 11,
      };
    }

    return [start, { yAxis: or(b.to, 0) }];
  });

  const seriesOption = values(series).map((s, i) => {
    const item = merge(
      {
        type: "line",
        smooth,
        showSymbol: false,
        lineStyle: { width: 3 },
      },
      s,
    );

    if (i === 0 && bandData.length) item.markArea = { silent: true, data: bandData };

    return item;
  });

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: { type: "category", data: categories, boundaryGap: false },
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/** WireCharts' `line`, as its view calls its builder. */
export const callLine = (p) =>
  cartesian("line", {
    series: p.series,
    categories: p.categories,
    smooth: p.smooth,
    step: p.step,
    stack: p.stack,
    legend: p.legend,
  });

/** WireCharts' `area`, as its view calls its builder. */
export const callArea = (p) =>
  cartesian("area", {
    series: p.series,
    categories: p.categories,
    smooth: p.smooth,
    stack: p.stack,
    legend: p.legend,
  });

/** WireCharts' `column`, as its view calls its builder. */
export const callColumn = (p) =>
  cartesian("column", { series: p.series, categories: p.categories, stack: p.stack, legend: p.legend });

/** WireCharts' `bar`, as its view calls its builder. */
export const callBar = (p) =>
  cartesian("bar", {
    series: p.series,
    categories: p.categories,
    stack: p.stack,
    legend: p.legend,
    horizontal: true,
  });

/** WireCharts' `audio`, as its view calls its builder. */
export const callAudio = (p) => cartesian("line", { series: p.series, categories: p.categories, smooth: true, legend: true });

/** WireCharts' `spline`, as its view calls its builder. */
export const callSpline = (p) =>
  spline({ series: p.series, categories: p.categories, legend: p.legend, symbolSize: p.symbolSize });

/** WireCharts' `spline-inverted`, as its view calls its builder. */
export const callSplineInverted = (p) =>
  splineInverted({ series: p.series, categories: p.categories, legend: p.legend, symbolSize: p.symbolSize });

/** WireCharts' `line-labels`, as its view calls its builder. */
export const callLineLabels = (p) => lineLabels({ series: p.series, categories: p.categories, smooth: p.smooth });

/** WireCharts' `line-log`, as its view calls its builder. */
export const callLineLog = (p) =>
  lineLog({
    series: p.series,
    categories: p.categories,
    smooth: p.smooth,
    legend: p.legend,
    logBase: p.logBase,
  });

/** WireCharts' `line-race`, as its view calls its builder. */
export const callLineRace = (p) => lineRace({ series: p.series, categories: p.categories, smooth: p.smooth });

/** WireCharts' `line-animated`, as its view calls its builder. */
export const callLineAnimated = (p) =>
  lineAnimated({
    series: p.series,
    categories: p.categories,
    smooth: p.smooth,
    legend: p.legend,
    duration: p.duration,
    easing: p.easing,
  });

/** WireCharts' `line-forecast`, as its view calls its builder. */
export const callLineForecast = (p) =>
  lineForecast({
    categories: p.categories,
    actual: p.actual,
    forecast: p.forecast,
    name: p.name,
    forecastName: p.forecastName,
    smooth: p.smooth,
  });

/** WireCharts' `line-annotated`, as its view calls its builder. */
export const callLineAnnotated = (p) =>
  lineAnnotated({
    series: p.series,
    categories: p.categories,
    annotations: p.annotations,
    smooth: p.smooth,
    legend: p.legend,
    average: p.average,
  });

/** WireCharts' `line-boost`, as its view calls its builder. */
export const callLineBoost = (p) =>
  lineBoost({
    series: p.series,
    categories: p.categories,
    sampling: p.sampling,
    zoom: p.zoom,
    legend: p.legend,
  });

/** WireCharts' `line-time`, as its view calls its builder. */
export const callLineTime = (p) => lineTime({ series: p.series, smooth: p.smooth, zoom: p.zoom, legend: p.legend });

/** WireCharts' `spline-time`, as its view calls its builder. */
export const callSplineTime = (p) =>
  splineTime({ series: p.series, zoom: p.zoom, legend: p.legend, symbolSize: p.symbolSize });

/** WireCharts' `spline-bands`, as its view calls its builder. */
export const callSplineBands = (p) =>
  splineBands({
    series: p.series,
    categories: p.categories,
    bands: p.bands,
    smooth: p.smooth,
    legend: p.legend,
  });

/** Every call here, by WireCharts' name (the tests'; a component imports its own). */
export const CALLS = {
  "line": callLine,
  "area": callArea,
  "column": callColumn,
  "bar": callBar,
  "audio": callAudio,
  "spline": callSpline,
  "spline-inverted": callSplineInverted,
  "line-labels": callLineLabels,
  "line-log": callLineLog,
  "line-race": callLineRace,
  "line-animated": callLineAnimated,
  "line-forecast": callLineForecast,
  "line-annotated": callLineAnnotated,
  "line-boost": callLineBoost,
  "line-time": callLineTime,
  "spline-time": callSplineTime,
  "spline-bands": callSplineBands,
};
