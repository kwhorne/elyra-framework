// Each chart's ECharts option, from its props and data: plain objects, so a
// test can read them (RFC 0058). The same options WireCharts builds in PHP,
// from the same props.

import { categoryText, formatter, isTime } from "./data.js";
import { themed } from "./theme.js";
import { marksOf, widened } from "./marks.js";
import { targetsOf, tooltipWithTargets } from "./targets.js";

/** What every chart shares: the theme, the tooltip, the legend, the grid. */
function base(theme, { legend = true, tooltip = true, trigger = "axis", format, locale, series }) {
  const t = themed(theme);
  const say = formatter(format, locale);
  const named = series.some((s) => s.name != null && s.name !== "");
  return {
    color: t.color,
    backgroundColor: t.backgroundColor,
    textStyle: t.textStyle,
    animation: t.animation,
    animationDuration: t.animationDuration,
    animationDurationUpdate: t.animationDurationUpdate,
    aria: { enabled: true, decal: { show: false } },
    tooltip: tooltip ? { ...t.tooltip, trigger, valueFormatter: say } : { show: false },
    legend: { ...t.legend, show: legend && named && series.length > 1, top: 0 },
    t,
  };
}

/**
 * With `zoom` (RFC 0061): the wheel, a pinch and a drag, and a slider, on
 * the category axis; ChartBase styles the slider and keeps the window.
 */
function zoomOf(zoom, sideways) {
  if (!zoom) return {};
  const axis = sideways ? { yAxisIndex: 0 } : { xAxisIndex: 0 };
  return { dataZoom: [{ type: "inside", ...axis }, { type: "slider", ...axis }] };
}

/** With marks, the legend lists the data's series only, not the marks' own. */
function legendOf(legend, shaped, marked) {
  if (marked.series.length === 0) return {};
  return { legend: { ...legend, data: shaped.map((s) => s.name).filter((n) => n != null && n !== "") } };
}

/** Line, area, column and bar: a category (or time) axis and a value one. */
export function cartesian(kind, { series, categories }, props, theme) {
  const { smooth = false, step = false, stack = false, percent = false, area = false, horizontal = false, zoom = false, locale, marks, targets } = props;
  const b = base(theme, { ...props, series });
  const { t, ...shared } = b;
  const bars = kind === "column" || kind === "bar";
  const filled = kind === "area" || area;
  const time = isTime(categories);
  const stacked = stack || percent ? (typeof stack === "string" ? stack : "total") : undefined;

  // A percent stack: each category's values as shares of its total.
  const totals = percent
    ? categories.map((_, i) => series.reduce((sum, s) => sum + (typeof s.data[i] === "number" ? s.data[i] : 0), 0))
    : null;
  const shaped = series.map((s) => {
    const { name, data, ...extra } = s;
    const values = totals ? data.map((v, i) => (typeof v === "number" && totals[i] ? (v / totals[i]) * 100 : v)) : data;
    return {
      type: bars ? "bar" : "line",
      ...(name != null ? { name } : {}),
      data: time ? values.map((v, i) => [categories[i], v]) : values,
      ...(smooth && !bars ? { smooth: true } : {}),
      ...(step && !bars ? { step: typeof step === "string" ? step : "start" } : {}),
      ...(stacked ? { stack: stacked } : {}),
      ...(filled ? { areaStyle: { opacity: stacked ? 0.85 : 0.25 } } : {}),
      ...(bars ? { barMaxWidth: 48 } : { showSymbol: data.length <= 24, connectNulls: false }),
      emphasis: { focus: "series" },
      ...extra,
    };
  });

  const categoryAxis = {
    type: time ? "time" : "category",
    ...(time ? {} : { data: categories.map((c) => categoryText(c, locale)) }),
    boundaryGap: bars,
    ...t.axis,
    splitLine: { show: false },
  };
  const valueAxis = {
    type: "value",
    ...t.axis,
    ...(percent ? { max: 100, axisLabel: { ...t.axis.axisLabel, formatter: "{value} %" } } : {}),
  };
  const sideways = kind === "bar" || horizontal;
  const zoomed = zoom && categories.length > 1;
  // Marks (RFC 0065): series of their own, and a value axis that takes them in.
  const marked = marksOf(marks, { categories, axisType: time ? "time" : "category", sideways, bars, theme, locale });
  // Targets (RFC 0072): a tick a category over columns, a step line over lines; not in a percent stack.
  const targeted = percent ? { series: [], extent: null, targets: null } : targetsOf(targets, { categories, time, sideways, bars, theme });
  const extents = [marked.extent, targeted.extent].filter(Boolean);
  const values = widened(valueAxis, extents.length ? [Math.min(...extents.map((e) => e[0])), Math.max(...extents.map((e) => e[1]))] : null);
  const extra = { series: [...marked.series, ...targeted.series] };
  return {
    ...shared,
    ...legendOf(shared.legend, shaped, extra),
    ...(targeted.targets && shared.tooltip.show !== false
      ? {
          tooltip: {
            ...shared.tooltip,
            formatter: tooltipWithTargets(targets, { series, categories, stack: !!stack, format: props.format, locale, words: props.words ?? { difference: "Difference" } }),
          },
        }
      : {}),
    grid: {
      left: 8,
      right: zoomed && sideways ? 40 : 16,
      bottom: zoomed && !sideways ? 44 : 8,
      top: shared.legend.show ? 36 : 12,
      containLabel: true,
    },
    xAxis: sideways ? values : categoryAxis,
    yAxis: sideways ? { ...categoryAxis, inverse: true } : values,
    series: [...shaped, ...marked.series, ...targeted.series],
    ...zoomOf(zoomed, sideways),
  };
}

/** A pie, or a donut: the first series' points, or one slice per category. */
export function pie({ series, categories }, props, theme) {
  const { donut = false, rose = false, legend = true, locale } = props;
  const b = base(theme, { ...props, series, trigger: "item" });
  const { t, ...shared } = b;
  const first = series[0] ?? { data: [] };
  const { name, data, ...extra } = first;
  const slices = data.map((p, i) => (p && typeof p === "object" ? p : { name: categoryText(categories[i], locale), value: p }));
  return {
    ...shared,
    legend: { ...t.legend, show: legend, bottom: 0, type: "scroll" },
    series: [
      {
        type: "pie",
        ...(name != null ? { name } : {}),
        radius: donut ? ["50%", "75%"] : [0, "75%"],
        center: ["50%", legend ? "45%" : "50%"],
        ...(rose ? { roseType: "radius" } : {}),
        itemStyle: { borderColor: t.tooltip.backgroundColor, borderWidth: 2 },
        label: { color: t.textStyle.color },
        data: slices,
        ...extra,
      },
    ],
  };
}

/** Points on two value axes; a third value is a bubble's size with `size`. */
export function scatter({ series }, props, theme) {
  const { size = false, zoom = false, marks, locale } = props;
  const b = base(theme, { ...props, series, trigger: "item" });
  const { t, ...shared } = b;
  const largest = size ? Math.max(1, ...series.flatMap((s) => s.data.map((p) => (Array.isArray(p) ? (p[2] ?? 0) : 0)))) : 1;
  // Marks (RFC 0065): `value` on y, `at` on x, both taken in by their axes.
  const marked = marksOf(marks, { axisType: "value", theme, locale });
  const xs = (Array.isArray(marks) ? marks : []).flatMap((m) => [m?.at].flat()).filter(Number.isFinite);
  const shaped = series.map(({ name, data, ...extra }) => ({
      type: "scatter",
      ...(name != null ? { name } : {}),
      data,
      symbolSize: size ? (p) => 8 + 32 * Math.sqrt((Array.isArray(p) ? (p[2] ?? 0) : 0) / largest) : 10,
      emphasis: { focus: "series" },
      ...extra,
    }));
  return {
    ...shared,
    ...legendOf(shared.legend, shaped, marked),
    grid: { left: 8, right: 16, bottom: zoom ? 44 : 8, top: shared.legend.show ? 36 : 12, containLabel: true },
    xAxis: widened({ type: "value", scale: true, ...t.axis }, xs.length ? [Math.min(...xs), Math.max(...xs)] : null),
    yAxis: widened({ type: "value", scale: true, ...t.axis }, marked.extent),
    series: [...shaped, ...marked.series],
    ...zoomOf(zoom, false),
  };
}

/** `<Chart option>`: the app's option over the theme's. */
export function custom(option, props, theme) {
  const b = base(theme, { ...props, series: option?.series ?? [] });
  const { t, ...shared } = b;
  void t;
  return { ...shared, ...option };
}
