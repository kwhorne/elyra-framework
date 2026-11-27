// WireCharts' area charts (RFC 0059): Option.php's area builders, ported as
// they are, and each component's call as its Blade view makes it.
import { or, truthy, isArray, values, merge, normalizeSeries } from "./helpers.js";

/** PHP's `[$a, $b, $c] = $x`: keys 0, 1, 2 of an array, null for anything else. */
const destructure = (value, count) =>
  Array.from({ length: count }, (_, i) => (isArray(value) ? or(value[i], null) : null));

/** PHP's string interpolation of a scalar: null and false as "", true as "1", floats at precision 14. */
function phpString(value) {
  if (value === null || value === undefined || value === false) return "";
  if (value === true) return "1";
  if (typeof value === "number" && !Number.isInteger(value) && Number.isFinite(value)) {
    return String(Number(value.toPrecision(14)));
  }
  return String(value);
}

/** PHP's `round($value, $precision)`: halves away from zero, after pre-rounding to 15 digits. */
function phpRound(value, precision = 0) {
  const factor = 10 ** precision;
  const scaled = Number((Math.abs(value) * factor).toPrecision(15));
  return (Math.sign(value) * Math.floor(scaled + 0.5)) / factor;
}

/** The low-to-high span per point, as the range builders stack it on the low series. */
function spans(low, high) {
  return low.map((l, i) => {
    const h = or(high[i], l);
    return l === null || h === null ? null : h - l;
  });
}

/** The value grid most area builders share, the top making room for a legend. */
const grid = (legend) => ({ left: "3%", right: "4%", bottom: "3%", top: truthy(legend) ? 40 : 16, containLabel: true });

/** The category axis along x, with no gap at the edges. */
const categoryAxis = (categories) => ({ type: "category", data: categories, boundaryGap: false });

/** PHP's `$stack !== false ? (is_string($stack) ? $stack : 'total')`, set on the item. */
function stacked(item, stack) {
  if (stack !== false) item.stack = typeof stack === "string" ? stack : "total";
  return item;
}

/**
 * Area chart with smooth gradient fills — each series fades from its colour
 * to transparent. Optionally stacked.
 */
export function areaGradient(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);
  const stack = or(args.stack, false);
  const legend = or(args.legend, true);

  const palette = [[249, 115, 22], [14, 165, 233], [34, 197, 94], [168, 85, 247], [239, 68, 68], [234, 179, 8]];

  const seriesOption = values(series).map((s, i) => {
    const [r, g, b] = palette[i % palette.length];
    const item = merge({
      type: "line",
      smooth,
      showSymbol: false,
      lineStyle: { width: 2, color: `rgb(${r},${g},${b})` },
      itemStyle: { color: `rgb(${r},${g},${b})` },
      areaStyle: {
        opacity: 1,
        color: {
          type: "linear", x: 0, y: 0, x2: 0, y2: 1,
          colorStops: [
            { offset: 0, color: `rgba(${r},${g},${b},0.45)` },
            { offset: 1, color: `rgba(${r},${g},${b},0.03)` },
          ],
        },
      },
    }, s);
    return stacked(item, stack);
  });

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/**
 * Stacked area chart: series are filled and stacked on top of one another
 * to show cumulative totals and composition.
 */
export function areaStacked(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, false);
  const legend = or(args.legend, true);

  const seriesOption = values(series).map((s) => merge({
    type: "line",
    stack: "total",
    smooth,
    showSymbol: false,
    lineStyle: { width: 1 },
    areaStyle: { opacity: 0.8 },
    emphasis: { focus: "series" },
  }, s));

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/**
 * 100% stacked area chart: each series is shown as its percentage share of
 * the total for every category.
 */
export function areaPercent(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, false);
  const legend = or(args.legend, true);

  const rows = values(series).map((s) => values(or(s.data, [])));
  const count = rows.length > 0 ? Math.max(...rows.map((row) => row.length)) : 0;

  for (let j = 0; j < count; j++) {
    let sum = 0;
    for (const row of rows) sum += or(row[j], 0);
    rows.forEach((row, i) => {
      const v = or(row[j], 0);
      rows[i][j] = sum > 0 ? phpRound((v / sum) * 100, 2) : 0;
    });
  }

  const seriesOption = values(series).map((s, i) => ({
    name: or(s.name, ""),
    type: "line",
    stack: "total",
    smooth,
    showSymbol: false,
    lineStyle: { width: 1 },
    areaStyle: { opacity: 0.8 },
    emphasis: { focus: "series" },
    data: rows[i],
  }));

  return {
    tooltip: { trigger: "axis", valueFormatter: (v) => v + "%" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: categoryAxis(categories),
    yAxis: { type: "value", max: 100, axisLabel: { formatter: "{value}%" } },
    series: seriesOption,
  };
}

/**
 * Area range: a filled band between a low and high value per category
 * (e.g. min/max temperature). Accepts low/high arrays or [low, high] pairs.
 */
export function areaRange(args) {
  const categories = or(args.categories, []);
  const low = values(or(args.low, []));
  const high = values(or(args.high, []));
  const name = or(args.name, "Range");
  const smooth = or(args.smooth, true);

  if (low.length === 0 && high.length === 0 && truthy(args.data)) {
    for (const pair of values(args.data)) {
      low.push(isArray(pair) ? or(pair[0], null) : null);
      high.push(isArray(pair) ? or(pair[1], null) : null);
    }
  }

  return {
    tooltip: { trigger: "axis" },
    legend: { show: false },
    grid: grid(false),
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series: [
      {
        name: "low",
        type: "line",
        stack: "range",
        smooth,
        showSymbol: false,
        silent: true,
        lineStyle: { opacity: 0 },
        areaStyle: { opacity: 0 },
        data: low,
      },
      {
        name,
        type: "line",
        stack: "range",
        smooth,
        showSymbol: false,
        lineStyle: { opacity: 0 },
        areaStyle: { opacity: 0.4 },
        data: spans(low, high),
      },
    ],
  };
}

/**
 * Area race: a multi-series filled area whose points are revealed
 * progressively (animated client-side), each labelled at its leading end.
 */
export function areaRace(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);

  const formatter = (params) => params.seriesName;

  const seriesOption = values(series).map((s) => merge({
    type: "line",
    smooth,
    showSymbol: false,
    lineStyle: { width: 2 },
    areaStyle: { opacity: 0.4 },
    emphasis: { focus: "series" },
    endLabel: { show: true, formatter, fontWeight: "bold" },
  }, s));

  return {
    tooltip: { trigger: "axis" },
    legend: { show: false },
    grid: { left: "3%", right: 90, bottom: "3%", top: 16, containLabel: true },
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/**
 * Area spline: a smooth filled area chart with semi-transparent overlapping
 * series. Optionally stacked.
 */
export function areaspline(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const stack = or(args.stack, false);
  const legend = or(args.legend, true);

  const seriesOption = values(series).map((s) => stacked(merge({
    type: "line",
    smooth: true,
    showSymbol: false,
    lineStyle: { width: 2 },
    areaStyle: { opacity: 0.4 },
  }, s), stack));

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/**
 * Area chart with inverted axes: filled areas drawn with the category axis
 * running vertically and the value axis horizontally.
 */
export function areaInverted(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);
  const legend = or(args.legend, true);

  const seriesOption = values(series).map((s) => merge({
    type: "line",
    smooth,
    showSymbol: false,
    lineStyle: { width: 2 },
    areaStyle: { opacity: 0.4 },
  }, s));

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: { type: "value" },
    yAxis: categoryAxis(categories),
    series: seriesOption,
  };
}

/**
 * Area chart with negative values: the line and fill are coloured
 * differently above and below the zero line.
 */
export function areaNegative(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);
  const legend = or(args.legend, false);
  const positive = or(args.positiveColor, "#22c55e");
  const negative = or(args.negativeColor, "#ef4444");

  const seriesOption = values(series).map((s) => merge({
    type: "line",
    smooth,
    showSymbol: false,
    lineStyle: { width: 2 },
    areaStyle: { opacity: 0.3 },
  }, s));

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    visualMap: {
      show: false,
      dimension: 1,
      pieces: [
        { lte: 0, color: negative },
        { gt: 0, color: positive },
      ],
    },
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/**
 * Area range with a line overlay: a filled low/high band plus a line
 * (e.g. the average) drawn on top.
 */
export function areaRangeLine(args) {
  const categories = or(args.categories, []);
  const low = values(or(args.low, []));
  const high = values(or(args.high, []));
  const line = values(or(args.line, []));
  const rangeName = or(args.rangeName, "Range");
  const lineName = or(args.lineName, "Average");
  const smooth = or(args.smooth, true);

  return {
    tooltip: { trigger: "axis" },
    legend: { show: true },
    grid: grid(true),
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series: [
      {
        name: "low",
        type: "line",
        stack: "range",
        smooth,
        showSymbol: false,
        silent: true,
        lineStyle: { opacity: 0 },
        areaStyle: { opacity: 0 },
        data: low,
      },
      {
        name: rangeName,
        type: "line",
        stack: "range",
        smooth,
        showSymbol: false,
        lineStyle: { opacity: 0 },
        areaStyle: { opacity: 0.3 },
        data: spans(low, high),
      },
      {
        name: lineName,
        type: "line",
        smooth,
        showSymbol: false,
        lineStyle: { width: 3 },
        data: line,
      },
    ],
  };
}

/**
 * Fan chart: a central line surrounded by nested confidence bands that fan
 * out with growing uncertainty. Each band has its own low/high arrays.
 */
export function areaFan(args) {
  const categories = or(args.categories, []);
  const line = values(or(args.line, []));
  const bands = or(args.bands, []);
  const lineName = or(args.lineName, "Median");
  const smooth = or(args.smooth, true);
  const [r, g, b] = destructure(or(args.color, [249, 115, 22]), 3).map(phpString);

  const series = [];

  values(bands).forEach((band, k) => {
    const low = values(or(band.low, []));
    const high = values(or(band.high, []));
    const opacity = or(band.opacity, 0.12 + k * 0.12);
    const stack = `fan${k}`;

    series.push({
      name: `fan-low-${k}`,
      type: "line",
      stack,
      smooth,
      showSymbol: false,
      silent: true,
      lineStyle: { opacity: 0 },
      areaStyle: { opacity: 0 },
      data: low,
    });
    series.push({
      name: or(band.name, `Band ${k + 1}`),
      type: "line",
      stack,
      smooth,
      showSymbol: false,
      lineStyle: { opacity: 0 },
      areaStyle: { color: `rgba(${r},${g},${b},${phpString(opacity)})` },
      data: spans(low, high),
    });
  });

  series.push({
    name: lineName,
    type: "line",
    smooth,
    showSymbol: false,
    lineStyle: { width: 2, color: `rgb(${r},${g},${b})` },
    data: line,
  });

  return {
    tooltip: { trigger: "axis" },
    legend: { show: false },
    grid: grid(false),
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series,
  };
}

/**
 * Streamgraph: a flowing stacked area displaced around a central baseline,
 * rendered with the themeRiver series.
 */
export function streamgraph(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, true);

  const names = [];
  const data = [];
  for (const s of values(series)) {
    const name = or(s.name, "");
    names.push(name);
    values(or(s.data, [])).forEach((v, i) => {
      data.push([or(categories[i], i), v, name]);
    });
  }

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "line" } },
    legend: { show: legend, data: names },
    singleAxis: {
      type: "category",
      data: categories,
      boundaryGap: false,
      top: 40,
      bottom: 30,
    },
    series: [{
      type: "themeRiver",
      emphasis: { focus: "self" },
      label: { show: false },
      data,
    }],
  };
}

/**
 * Stacked area chart with inverted axes: filled, stacked series drawn with
 * the category axis running vertically.
 */
export function areaStackedInverted(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, false);
  const legend = or(args.legend, true);

  const seriesOption = values(series).map((s) => merge({
    type: "line",
    stack: "total",
    smooth,
    showSymbol: false,
    lineStyle: { width: 1 },
    areaStyle: { opacity: 0.8 },
    emphasis: { focus: "series" },
  }, s));

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: { type: "value" },
    yAxis: categoryAxis(categories),
    series: seriesOption,
  };
}

/**
 * Area chart with missing points: null values leave a gap, or are bridged
 * when connectNulls is enabled.
 */
export function areaMissing(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const smooth = or(args.smooth, true);
  const connectNulls = or(args.connectNulls, false);
  const legend = or(args.legend, true);

  const seriesOption = values(series).map((s) => merge({
    type: "line",
    smooth,
    connectNulls,
    showSymbol: true,
    symbolSize: 5,
    lineStyle: { width: 2 },
    areaStyle: { opacity: 0.4 },
  }, s));

  return {
    tooltip: { trigger: "axis" },
    legend: { show: legend },
    grid: grid(legend),
    xAxis: categoryAxis(categories),
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/** WireCharts' `area-gradient`, as its view calls its builder. */
export const callAreaGradient = (p) => areaGradient({
  series: p.series, categories: p.categories, smooth: p.smooth, stack: p.stack, legend: p.legend,
});

/** WireCharts' `area-stacked`, as its view calls its builder. */
export const callAreaStacked = (p) => areaStacked({
  series: p.series, categories: p.categories, smooth: p.smooth, legend: p.legend,
});

/** WireCharts' `area-percent`, as its view calls its builder. */
export const callAreaPercent = (p) => areaPercent({
  series: p.series, categories: p.categories, smooth: p.smooth, legend: p.legend,
});

/** WireCharts' `area-range`, as its view calls its builder. */
export const callAreaRange = (p) => areaRange({
  categories: p.categories, low: p.low, high: p.high, data: p.data, name: p.name, smooth: p.smooth,
});

/** WireCharts' `area-race`, as its view calls its builder. */
export const callAreaRace = (p) => areaRace({
  series: p.series, categories: p.categories, smooth: p.smooth,
});

/** WireCharts' `areaspline`, as its view calls its builder. */
export const callAreaspline = (p) => areaspline({
  series: p.series, categories: p.categories, stack: p.stack, legend: p.legend,
});

/** WireCharts' `area-inverted`, as its view calls its builder. */
export const callAreaInverted = (p) => areaInverted({
  series: p.series, categories: p.categories, smooth: p.smooth, legend: p.legend,
});

/** WireCharts' `area-negative`, as its view calls its builder. */
export const callAreaNegative = (p) => areaNegative({
  series: p.series, categories: p.categories, smooth: p.smooth, legend: p.legend,
  positiveColor: p.positiveColor, negativeColor: p.negativeColor,
});

/** WireCharts' `area-range-line`, as its view calls its builder. */
export const callAreaRangeLine = (p) => areaRangeLine({
  categories: p.categories, low: p.low, high: p.high, line: p.line,
  rangeName: p.rangeName, lineName: p.lineName, smooth: p.smooth,
});

/** WireCharts' `area-fan`, as its view calls its builder. */
export const callAreaFan = (p) => areaFan({
  categories: p.categories, line: p.line, bands: p.bands,
  lineName: p.lineName, smooth: p.smooth, color: p.color,
});

/** WireCharts' `streamgraph`, as its view calls its builder. */
export const callStreamgraph = (p) => streamgraph({
  series: p.series, categories: p.categories, legend: p.legend,
});

/** WireCharts' `area-stacked-inverted`, as its view calls its builder. */
export const callAreaStackedInverted = (p) => areaStackedInverted({
  series: p.series, categories: p.categories, smooth: p.smooth, legend: p.legend,
});

/** WireCharts' `area-missing`, as its view calls its builder. */
export const callAreaMissing = (p) => areaMissing({
  series: p.series, categories: p.categories, smooth: p.smooth,
  connectNulls: p.connectNulls, legend: p.legend,
});

/** Every call here, by WireCharts' name (the tests'; a component imports its own). */
export const CALLS = {
  "area-gradient": callAreaGradient,
  "area-stacked": callAreaStacked,
  "area-percent": callAreaPercent,
  "area-range": callAreaRange,
  "area-race": callAreaRace,
  "areaspline": callAreaspline,
  "area-inverted": callAreaInverted,
  "area-negative": callAreaNegative,
  "area-range-line": callAreaRangeLine,
  "area-fan": callAreaFan,
  "streamgraph": callStreamgraph,
  "area-stacked-inverted": callAreaStackedInverted,
  "area-missing": callAreaMissing,
};
