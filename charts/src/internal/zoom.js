// Zoom (RFC 0061): the window on a long series, as the axis's own values
// (`["2026-03-01", "2026-03-31"]`, `["Q2", "Q3"]`) to and from ECharts'
// percents; the keys that move it; what's shown. Plain functions over the
// option, so the six of 0.58 and WireCharts' zoomable three share them.

import { categoryText, formatter } from "./data.js";

const EPS = 1e-6;
const list = (v) => (Array.isArray(v) ? v : v === undefined || v === null ? [] : [v]);
const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));
/** A point's x: `[x, y]`, `{ value: [x, y] }`. */
const xOf = (p) => (Array.isArray(p) ? p[0] : Array.isArray(p?.value) ? p.value[0] : undefined);
/** A time as milliseconds: a number, a `Date`, an ISO string. */
const msOf = (v) => (typeof v === "number" ? v : v instanceof Date ? v.getTime() : Date.parse(v));

/** Whether the option zooms: it has a data zoom. */
export const zooms = (option) => list(option?.dataZoom).length > 0;

/** Whether a window is all of the axis. */
export const isAll = (win) => !win || (win.start <= EPS && win.end >= 100 - EPS);

/**
 * The axis the window is on, as `{ type, values, min, max, vertical }`: a
 * category axis's values are its categories; a time axis's, the first
 * series' times; a value axis's (a scatter's x), every point's x, in the
 * order the table lists them.
 */
export function axisOf(option) {
  const xs = list(option?.xAxis);
  const ys = list(option?.yAxis);
  const vertical = ys[0]?.type === "category" && xs[0]?.type !== "category";
  const axis = (vertical ? ys : xs)[0] ?? {};
  const series = list(option?.series);
  if (axis.type === "category" || (axis.type === undefined && Array.isArray(axis.data))) {
    const values = list(axis.data).map((c) => (c && typeof c === "object" ? c.value : c));
    return { type: "category", values, min: 0, max: Math.max(0, values.length - 1), vertical };
  }
  if (axis.type === "time") {
    const values = list(series[0]?.data).map(xOf).filter((x) => x !== undefined && x !== null);
    const ms = values.map(msOf).filter(Number.isFinite);
    return { type: "time", values, min: Math.min(...ms), max: Math.max(...ms), vertical };
  }
  const values = series.flatMap((s) => list(s.data).map(xOf)).filter((x) => typeof x === "number");
  return { type: "value", values, min: Math.min(...values), max: Math.max(...values), vertical };
}

/** Where a value lies on the axis, as a percent of it; NaN if it isn't on it. */
export function percentOf(axis, value) {
  if (axis.type === "category") {
    const i = axis.values.findIndex((v) => String(v) === String(value));
    if (i < 0) return NaN;
    return axis.max === 0 ? 0 : (i / axis.max) * 100;
  }
  const x = axis.type === "time" ? msOf(value) : Number(value);
  if (!Number.isFinite(x) || !Number.isFinite(axis.min)) return NaN;
  return axis.max === axis.min ? 0 : clamp(((x - axis.min) / (axis.max - axis.min)) * 100, 0, 100);
}

/**
 * The index of the point nearest `value` on the axis: its category, or the
 * nearest time or x. A category that isn't there, between dates, is the
 * nearest date; else -1.
 */
export function nearestIndex(axis, value) {
  if (!axis || axis.values.length === 0) return -1;
  if (axis.type === "category") {
    const at = axis.values.findIndex((v) => String(v) === String(value));
    if (at >= 0) return at;
    const target = msOf(value);
    const times = axis.values.map(msOf);
    if (!Number.isFinite(target) || !times.every(Number.isFinite)) return -1;
    return closest(times, target);
  }
  const at = axis.type === "time" ? msOf : Number;
  const target = at(value);
  if (!Number.isFinite(target)) return -1;
  return closest(axis.values.map(at), target);
}

function closest(xs, target) {
  let best = 0;
  for (let i = 1; i < xs.length; i++) if (Math.abs(xs[i] - target) < Math.abs(xs[best] - target)) best = i;
  return best;
}

/** A range on another chart's axis as this one's: each end snapped to the nearest value it has. */
export function snapRange(axis, range) {
  if (!axis || !Array.isArray(range) || axis.type !== "category") return range;
  const ends = range.map((v) => nearestIndex(axis, v));
  return ends.every((i) => i >= 0) ? ends.map((i) => axis.values[i]) : range;
}

/** Where the `i`th point lies, as a percent. */
export function percentAt(axis, i) {
  if (axis.type === "category") return axis.max === 0 ? 0 : (clamp(i, 0, axis.max) / axis.max) * 100;
  return percentOf(axis, axis.values[i]);
}

const significant = (x) => Number(x.toPrecision(6));

/** The window as the axis's values, `[from, to]`, or null for all of it. */
export function rangeOf(axis, win) {
  if (!axis || isAll(win) || axis.values.length === 0) return null;
  if (axis.type === "category") {
    const at = (p) => axis.values[clamp(Math.round((p / 100) * axis.max), 0, axis.max)];
    return [at(win.start), at(win.end)];
  }
  const span = axis.max - axis.min;
  const lo = axis.min + (win.start / 100) * span;
  const hi = axis.min + (win.end / 100) * span;
  if (axis.type === "value") return [significant(lo), significant(hi)];
  // A time: the first point in the window and the last, as they were given.
  const slack = span * EPS;
  const inside = axis.values.filter((v) => msOf(v) >= lo - slack && msOf(v) <= hi + slack);
  if (inside.length) return [inside[0], inside[inside.length - 1]];
  const nearest = axis.values.reduce((best, v) => (Math.abs(msOf(v) - lo) < Math.abs(msOf(best) - lo) ? v : best));
  return [nearest, nearest];
}

/** A range as ECharts' percents; all of it for null, or a range not on the axis. */
export function windowOf(axis, range) {
  if (!axis || !Array.isArray(range) || range.length !== 2) return { start: 0, end: 100 };
  let start = percentOf(axis, range[0]);
  let end = percentOf(axis, range[1]);
  if (!Number.isFinite(start)) start = 0;
  if (!Number.isFinite(end)) end = 100;
  return start <= end ? { start, end } : { start: end, end: start };
}

/** Two ranges the same. */
export const sameRange = (a, b) =>
  a === b || (Array.isArray(a) && Array.isArray(b) && String(a[0]) === String(b[0]) && String(a[1]) === String(b[1]));

/**
 * The window after new data: a window at the axis's end follows it (the
 * last 30 days stay the last 30), one elsewhere keeps its values.
 */
export function follow(before, axis, win, range) {
  if (!range) return { start: 0, end: 100 };
  if (before && win && win.end >= 100 - EPS && win.start > EPS) {
    if (axis.type === "category") {
      const shown = ((win.end - win.start) / 100) * before.max;
      return { start: axis.max === 0 ? 0 : clamp(((axis.max - shown) / axis.max) * 100, 0, 100), end: 100 };
    }
    const span = ((win.end - win.start) / 100) * (before.max - before.min);
    const width = axis.max - axis.min;
    return { start: width ? clamp(((axis.max - span - axis.min) / width) * 100, 0, 100) : 0, end: 100 };
  }
  return windowOf(axis, range);
}

/** One step along the axis, as a percent: a category, or a hundredth. */
export const stepOf = (axis) => (axis?.type === "category" && axis.max > 0 ? 100 / axis.max : 1);

/** Zoom in (`factor` < 1) or out about `focus` (a percent; the middle if none). */
export function zoomBy(axis, win, factor, focus) {
  const width = Math.max(win.end - win.start, EPS);
  const least = axis?.type === "category" ? Math.min(100, stepOf(axis)) : 1;
  const next = clamp(width * factor, least, 100);
  const f = Number.isFinite(focus) ? clamp(focus, win.start, win.end) : (win.start + win.end) / 2;
  const start = clamp(f - (f - win.start) * (next / width), 0, 100 - next);
  return { start, end: start + next };
}

/** The window moved by `delta` percent, kept on the axis. */
export function panBy(win, delta) {
  const width = win.end - win.start;
  const start = clamp(win.start + delta, 0, 100 - width);
  return { start, end: start + width };
}

/** The window moved as little as it takes to show `at` (a percent). */
export function keepInView(win, at) {
  if (at < win.start - EPS) return panBy(win, at - win.start);
  if (at > win.end + EPS) return panBy(win, at - win.end);
  return win;
}

/** Whether the `i`th point (or row) is in the range: its index on a category axis, its x else. */
export function shownBy(axis, range) {
  if (!axis || !range) return () => true;
  if (axis.type === "category") {
    const w = windowOf(axis, range);
    const from = Math.round((w.start / 100) * axis.max);
    const to = Math.round((w.end / 100) * axis.max);
    return (i) => i >= from && i <= to;
  }
  const at = axis.type === "time" ? msOf : Number;
  const [lo, hi] = [at(range[0]), at(range[1])];
  return (i) => {
    const x = at(axis.values[i]);
    return x >= lo && x <= hi;
  };
}

/** A point's x in the range (a time or value axis). */
function inRange(axis, range) {
  const at = axis.type === "time" ? msOf : Number;
  const [lo, hi] = [at(range[0]), at(range[1])];
  return (p) => {
    const x = at(xOf(p));
    return x >= lo && x <= hi;
  };
}

/** The six's data with only the window's points, for what the chart says. */
export function sliceData(data, kind, axis, range) {
  if (!axis || !range) return data;
  if (kind === "scatter") {
    const keep = inRange(axis, range);
    return { ...data, series: data.series.map((s) => ({ ...s, data: s.data.filter(keep) })) };
  }
  const shown = shownBy(axis, range);
  const pick = (values) => values.filter((_, i) => shown(i));
  return { categories: pick(data.categories), series: data.series.map((s) => ({ ...s, data: pick(s.data) })) };
}

/** An option with only the window's points, for what a WireCharts chart says. */
export function sliceOption(option, axis, range) {
  if (!axis || !range) return option;
  const key = axis.vertical ? "yAxis" : "xAxis";
  if (axis.type === "category") {
    const shown = shownBy(axis, range);
    const pick = (values) => list(values).filter((_, i) => shown(i));
    const axes = list(option[key]).map((a, i) => (i === 0 ? { ...a, data: pick(a.data) } : a));
    return { ...option, [key]: axes, series: list(option.series).map((s) => ({ ...s, data: pick(s.data) })) };
  }
  const keep = inRange(axis, range);
  return { ...option, series: list(option.series).map((s) => ({ ...s, data: list(s.data).filter(keep) })) };
}

/** A range's end as text: a date in the locale's words, a number by the chart's format. */
export function textOf(axis, value, format, locale) {
  if (axis?.type === "value") return formatter(format, locale)(value);
  if (axis?.type === "time" && typeof value === "number") {
    return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone: "UTC" }).format(new Date(value));
  }
  return categoryText(value, locale);
}

/** Where the slider is drawn: the overlay's handles are put on it. */
export const SLIDER = { horizontal: { left: 48, right: 48, bottom: 8, height: 22 }, vertical: { right: 8, top: 16, bottom: 16, width: 22 } };

/** The data zooms in the tokens' colours, the slider where the overlay expects it, at `win`. */
export function styleZoom(option, theme, axis, win) {
  const vertical = axis?.vertical ?? false;
  const slider = {
    ...(vertical ? { orient: "vertical", ...SLIDER.vertical } : { orient: "horizontal", ...SLIDER.horizontal }),
    backgroundColor: theme.inset,
    borderColor: theme.border,
    fillerColor: theme.accentSoft,
    dataBackground: { lineStyle: { color: theme.muted, opacity: 0.6 }, areaStyle: { color: theme.muted, opacity: 0.15 } },
    selectedDataBackground: { lineStyle: { color: theme.accent }, areaStyle: { color: theme.accent, opacity: 0.2 } },
    handleStyle: { color: theme.accent, borderColor: theme.accent },
    moveHandleStyle: { color: theme.accent, opacity: 0.6 },
    emphasis: { handleStyle: { color: theme.accent, borderColor: theme.text } },
    textStyle: { color: theme.muted },
    brushSelect: false,
  };
  const at = win ? { start: win.start, end: win.end } : {};
  return {
    ...option,
    dataZoom: list(option.dataZoom).map((dz) => {
      const own = { ...dz, ...at };
      // A WireCharts builder gives none: the category axis's, then.
      if (dz.xAxisIndex === undefined && dz.yAxisIndex === undefined) own[vertical ? "yAxisIndex" : "xAxisIndex"] = 0;
      return dz.type === "slider" ? { ...own, ...slider } : own;
    }),
  };
}
