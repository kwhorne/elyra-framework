// WireCharts' column and bar builders (RFC 0059), ported from Option.php's
// stackedColumns through pareto: each returns the option Option.php returns
// for the same arguments, with real functions where PHP writes "@@function …@@".
import { or, truthy, isArray, values, merge, normalizeSeries } from "./helpers.js";

/** PHP's `is_numeric`: ints, floats and numeric strings (leading/trailing whitespace allowed). */
function isNumeric(value) {
  if (typeof value === "number") return true;
  if (typeof value !== "string") return false;
  return /^[ \t\n\r\v\f]*[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?[ \t\n\r\v\f]*$/.test(value);
}

/** A value as PHP's arithmetic reads it: numbers as they are, numeric strings parsed, null/false 0, true 1. */
function num(value) {
  if (typeof value === "number") return value;
  if (value === null || value === undefined || value === false) return 0;
  if (value === true) return 1;
  if (typeof value === "string") {
    const lead = value.match(/^[ \t\n\r\v\f]*[+-]?(\d+(\.\d*)?|\.\d+)([eE][+-]?\d+)?/);
    return lead ? Number(lead[0]) : 0;
  }
  return 0;
}

/** PHP's `(int)`: numbers truncated, strings by their leading number. */
function toInt(value) {
  const n = num(value);
  return Number.isFinite(n) ? Math.trunc(n) : 0;
}

/** PHP's `round($value, $places)`: half away from zero, after pre-rounding to 15 digits. */
function round(value, places = 0) {
  const n = num(value);
  if (!Number.isFinite(n)) return n;
  const f = 10 ** places;
  const scaled = Number((Math.abs(n) * f).toPrecision(15));
  if (!Number.isFinite(scaled) || scaled >= 1e15) return n;
  const rounded = Math.round(scaled);
  return (n < 0 ? -rounded : rounded) / f;
}

/** A float as PHP writes it into a string (`precision` 14, `%.14G`). */
function floatString(value) {
  if (Number.isNaN(value)) return "NAN";
  if (!Number.isFinite(value)) return value < 0 ? "-INF" : "INF";
  if (value === 0) return Object.is(value, -0) ? "-0" : "0";
  const [mantissa, exp] = value.toExponential(13).split("e");
  const exponent = Number(exp);
  if (exponent < -4 || exponent >= 14) {
    let m = String(Number(mantissa));
    if (!m.includes(".")) m += ".0";
    return `${m}E${exponent < 0 ? "-" : "+"}${Math.abs(exponent)}`;
  }
  return String(Number(value.toPrecision(14)));
}

/** PHP 8's `<=>`: numbers and numeric strings numerically, else as JS compares them. */
function spaceship(a, b) {
  if (isNumeric(a) && isNumeric(b)) {
    a = num(a);
    b = num(b);
  }
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Stacked columns or bars, optionally normalised to 100%. */
function stackedColumns(args, horizontal, percent) {
  let series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, true);

  if (percent) {
    const rows = values(series).map((s) => values(or(s.data, [])));
    const count = truthy(rows) ? Math.max(...rows.map((row) => row.length)) : 0;
    for (let j = 0; j < count; j++) {
      let sum = 0;
      for (const row of rows) sum += num(or(row[j], 0));
      rows.forEach((row, i) => {
        const v = or(row[j], 0);
        rows[i][j] = sum > 0 ? round((num(v) / sum) * 100, 2) : 0;
      });
    }
    series = values(series).map((s, i) => merge(s, { data: rows[i] }));
  }

  const seriesOption = values(series).map((s) =>
    merge({ type: "bar", stack: "total", emphasis: { focus: "series" } }, s),
  );

  const cat = { type: "category", data: categories };
  const val = percent
    ? { type: "value", max: 100, axisLabel: { formatter: "{value}%" } }
    : { type: "value" };

  const tooltip = { trigger: "axis", axisPointer: { type: "shadow" } };
  if (percent) {
    tooltip.valueFormatter = function (v) {
      return v + "%";
    };
  }

  return {
    tooltip,
    legend: { show: legend },
    grid: { left: "3%", right: "4%", bottom: "3%", top: truthy(legend) ? 40 : 16, containLabel: true },
    xAxis: horizontal ? val : cat,
    yAxis: horizontal ? cat : val,
    series: seriesOption,
  };
}

/** Stacked column chart: vertical bars stacked to show totals and composition. */
export function columnStacked(args) {
  return stackedColumns(args, false, false);
}

/** Stacked bar chart: horizontal bars stacked to show totals. */
export function barStacked(args) {
  return stackedColumns(args, true, false);
}

/** Percentage column chart: 100% stacked columns. */
export function columnPercent(args) {
  return stackedColumns(args, false, true);
}

/** Percentage bar chart: 100% stacked horizontal bars. */
export function barPercent(args) {
  return stackedColumns(args, true, true);
}

/** Column chart with positive/negative bars coloured by sign. */
export function columnNegative(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, false);
  const positive = or(args.positiveColor, "#22c55e");
  const negative = or(args.negativeColor, "#ef4444");

  const color = function (p) {
    return p.value < 0 ? negative : positive;
  };

  const seriesOption = values(series).map((s) =>
    merge({ type: "bar", itemStyle: { color, borderRadius: [3, 3, 0, 0] } }, s),
  );

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
    legend: { show: legend },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 16, containLabel: true },
    xAxis: { type: "category", data: categories },
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/** Column chart with rotated category labels, for many or long labels. */
export function columnRotated(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const legend = or(args.legend, true);
  const rotate = or(args.rotate, 45);

  const seriesOption = values(series).map((s) =>
    merge({ type: "bar", emphasis: { focus: "series" } }, s),
  );

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
    legend: { show: legend },
    grid: { left: "3%", right: "4%", bottom: "3%", top: truthy(legend) ? 40 : 16, containLabel: true },
    xAxis: { type: "category", data: categories, axisLabel: { rotate, interval: 0 } },
    yAxis: { type: "value" },
    series: seriesOption,
  };
}

/** Floating range bars from a low to a high value (vertical or horizontal). */
function rangeColumns(args, horizontal) {
  const categories = or(args.categories, []);
  const low = values(or(args.low, []));
  const high = values(or(args.high, []));
  const name = or(args.name, "Range");

  const diff = low.map((l, i) => {
    const h = or(high[i], l);
    return l === null || l === undefined || h === null || h === undefined ? null : num(h) - num(l);
  });

  const cat = { type: "category", data: categories };
  const val = { type: "value" };

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
    legend: { show: false },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 16, containLabel: true },
    xAxis: horizontal ? val : cat,
    yAxis: horizontal ? cat : val,
    series: [
      {
        name: "low",
        type: "bar",
        stack: "range",
        silent: true,
        itemStyle: { color: "transparent" },
        data: low,
      },
      {
        name,
        type: "bar",
        stack: "range",
        itemStyle: { borderRadius: 4 },
        data: diff,
      },
    ],
  };
}

/** Column range chart: floating vertical bars from low to high per category. */
export function columnRange(args) {
  return rangeColumns(args, false);
}

/** Bar range chart: floating horizontal bars from low to high. */
export function barRange(args) {
  return rangeColumns(args, true);
}

/** Histogram: bins a flat list of numeric values into frequency columns. */
export function histogram(args) {
  const data = values(or(args.data, [])).filter(isNumeric).map(num);
  const bins = Math.max(1, toInt(or(args.bins, 10)));

  const labels = [];
  const counts = new Array(bins).fill(0);

  if (data.length > 0) {
    const min = Math.min(...data);
    const max = Math.max(...data);
    const width = max - min || 1;
    const step = width / bins;

    for (const v of data) {
      let idx = Math.trunc(Math.floor((v - min) / step));
      idx = Math.max(0, Math.min(bins - 1, idx));
      counts[idx]++;
    }
    for (let i = 0; i < bins; i++) {
      const lo = round(min + i * step, 1);
      const hi = round(min + (i + 1) * step, 1);
      labels.push(`${floatString(lo)}–${floatString(hi)}`);
    }
  }

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
    legend: { show: false },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 16, containLabel: true },
    xAxis: { type: "category", data: labels },
    yAxis: { type: "value" },
    series: [
      {
        type: "bar",
        barCategoryGap: "1%",
        data: counts,
      },
    ],
  };
}

/** Lollipop chart: a thin stick topped with a circular marker per value. */
export function lollipop(args) {
  const series = normalizeSeries(or(args.series, []));
  const categories = or(args.categories, []);
  const first = series[0];
  const data = isArray(first) ? or(first.data, []) : [];

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
    legend: { show: false },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 16, containLabel: true },
    xAxis: { type: "category", data: categories },
    yAxis: { type: "value" },
    series: [
      { type: "bar", barWidth: 2, silent: true, data },
      { type: "scatter", symbolSize: 16, data },
    ],
  };
}

/** Pareto chart: columns sorted descending with a cumulative percentage line. */
export function pareto(args) {
  const categories = or(args.categories, []);
  const data = values(or(args.data, []));

  const pairs = data.map((v, i) => [or(categories[i], i), v]);
  pairs.sort((a, b) => spaceship(b[1], a[1]));

  const cats = pairs.map((p) => p[0]);
  const vals = pairs.map((p) => p[1]);
  const total = vals.reduce((sum, v) => sum + num(v), 0) || 1;

  const cumulative = [];
  let running = 0;
  for (const v of vals) {
    running += num(v);
    cumulative.push(round((running / total) * 100, 1));
  }

  return {
    tooltip: { trigger: "axis", axisPointer: { type: "shadow" } },
    legend: { show: true },
    grid: { left: "3%", right: "4%", bottom: "3%", top: 40, containLabel: true },
    xAxis: { type: "category", data: cats },
    yAxis: [
      { type: "value" },
      { type: "value", max: 100, axisLabel: { formatter: "{value}%" }, splitLine: { show: false } },
    ],
    series: [
      { name: or(args.name, "Count"), type: "bar", data: vals },
      { name: "Cumulative", type: "line", yAxisIndex: 1, smooth: false, symbolSize: 6, data: cumulative },
    ],
  };
}

/** WireCharts' `column-stacked`, as its view calls its builder. */
export const callColumnStacked = (props) =>
  columnStacked({ series: props.series, categories: props.categories, legend: props.legend });

/** WireCharts' `bar-stacked`, as its view calls its builder. */
export const callBarStacked = (props) =>
  barStacked({ series: props.series, categories: props.categories, legend: props.legend });

/** WireCharts' `column-percent`, as its view calls its builder. */
export const callColumnPercent = (props) =>
  columnPercent({ series: props.series, categories: props.categories, legend: props.legend });

/** WireCharts' `bar-percent`, as its view calls its builder. */
export const callBarPercent = (props) =>
  barPercent({ series: props.series, categories: props.categories, legend: props.legend });

/** WireCharts' `column-negative`, as its view calls its builder. */
export const callColumnNegative = (props) =>
  columnNegative({
    series: props.series,
    categories: props.categories,
    legend: props.legend,
    positiveColor: props.positiveColor,
    negativeColor: props.negativeColor,
  });

/** WireCharts' `column-rotated`, as its view calls its builder. */
export const callColumnRotated = (props) =>
  columnRotated({
    series: props.series,
    categories: props.categories,
    legend: props.legend,
    rotate: props.rotate,
  });

/** WireCharts' `column-range`, as its view calls its builder. */
export const callColumnRange = (props) =>
  columnRange({ categories: props.categories, low: props.low, high: props.high, name: props.name });

/** WireCharts' `bar-range`, as its view calls its builder. */
export const callBarRange = (props) =>
  barRange({ categories: props.categories, low: props.low, high: props.high, name: props.name });

/** WireCharts' `histogram`, as its view calls its builder. */
export const callHistogram = (props) => histogram({ data: props.data, bins: props.bins });

/** WireCharts' `lollipop`, as its view calls its builder. */
export const callLollipop = (props) => lollipop({ series: props.series, categories: props.categories });

/** WireCharts' `pareto`, as its view calls its builder. */
export const callPareto = (props) => pareto({ categories: props.categories, data: props.data, name: props.name });

/** Every call here, by WireCharts' name (the tests'; a component imports its own). */
export const CALLS = {
  "column-stacked": callColumnStacked,
  "bar-stacked": callBarStacked,
  "column-percent": callColumnPercent,
  "bar-percent": callBarPercent,
  "column-negative": callColumnNegative,
  "column-rotated": callColumnRotated,
  "column-range": callColumnRange,
  "bar-range": callBarRange,
  "histogram": callHistogram,
  "lollipop": callLollipop,
  "pareto": callPareto,
};
