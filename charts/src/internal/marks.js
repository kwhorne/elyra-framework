// Marks (RFC 0065): a target, a threshold, a period, drawn on a chart and
// said in its summary. `{ value }` is on the value axis, `{ at }` on the
// category (or time, or a scatter's x) axis; one is a line, two a band.
// Plain functions over the props, so a test reads them without a layout.

import { categoryText, formatter, valueOf } from "./data.js";
import { withAlpha } from "./brush.js";

/** The ids of the series that carry the marks: never data. */
export const MARK_AREAS = "elyra-marks-areas";
export const MARK_LINES = "elyra-marks-lines";

/** Whether an ECharts series is one of the marks'. */
export const isMarkSeries = (s) => s?.id === MARK_AREAS || s?.id === MARK_LINES;

const TONES = ["muted", "accent", "success", "warning", "danger"];
const pair = (v) => Array.isArray(v) && v.length === 2;
/** A time as milliseconds: a number, a `Date`, an ISO string. */
const msOf = (v) => (typeof v === "number" ? v : v instanceof Date ? v.getTime() : Date.parse(v));

/**
 * The marks the app gave, as `{ on, from, to, band, label, tone }`: `on`
 * is "value" or "at", a line has `from === to`. Anything else is dropped.
 */
export function marksFrom(marks) {
  if (!Array.isArray(marks)) return [];
  return marks.flatMap((m) => {
    if (!m || typeof m !== "object") return [];
    const on = m.value !== undefined && m.value !== null ? "value" : m.at !== undefined && m.at !== null ? "at" : null;
    if (!on) return [];
    const given = m[on];
    if (on === "value" && !(pair(given) ? given.every(Number.isFinite) : Number.isFinite(given))) return [];
    const [from, to] = pair(given) ? given : [given, given];
    const ordered = on === "value" && from > to ? [to, from] : [from, to];
    return [
      {
        on,
        from: ordered[0],
        to: ordered[1],
        band: pair(given),
        label: m.label == null || m.label === "" ? null : String(m.label),
        tone: TONES.includes(m.tone) ? m.tone : "muted",
      },
    ];
  });
}

/** The nearest category's index to `value`, by text or by date. */
function nearestCategory(categories, value, locale) {
  const text = categoryText(value, locale);
  const at = categories.findIndex((c) => c === value || categoryText(c, locale) === text);
  if (at >= 0) return at;
  const target = msOf(value);
  const times = categories.map(msOf);
  if (!Number.isFinite(target) || !times.every(Number.isFinite) || times.length === 0) return -1;
  let best = 0;
  for (let i = 1; i < times.length; i++) if (Math.abs(times[i] - target) < Math.abs(times[best] - target)) best = i;
  return best;
}

/**
 * Where an `at` mark's ends are on the axis, as ECharts takes them: a
 * category's index (a band over bars takes in the bars at its ends, half a
 * step out), a time's milliseconds, a scatter's x. Null if it isn't there.
 */
function atCoords(mark, { categories, axisType, bars, locale }) {
  if (axisType === "category") {
    const a = nearestCategory(categories, mark.from, locale);
    const b = nearestCategory(categories, mark.to, locale);
    if (a < 0 || b < 0) return null;
    const [lo, hi] = a <= b ? [a, b] : [b, a];
    return mark.band && bars ? [lo - 0.5, hi + 0.5] : [lo, hi];
  }
  if (axisType === "time") {
    const [a, b] = [msOf(mark.from), msOf(mark.to)];
    return Number.isFinite(a) && Number.isFinite(b) ? [Math.min(a, b), Math.max(a, b)] : null;
  }
  const [a, b] = [Number(mark.from), Number(mark.to)];
  return Number.isFinite(a) && Number.isFinite(b) ? [Math.min(a, b), Math.max(a, b)] : null;
}

/**
 * The marks as ECharts draws them: `series`, two of their own (the bands
 * behind the data, the lines over it), and `extent`, what the value axis
 * must take in (`dataMin`/`dataMax`) so a mark beyond the data is seen.
 *
 * `axisType` is the category axis's type ("category", "time", or "value"
 * for a scatter's x); `sideways` turns the axes (a `BarChart`).
 */
export function marksOf(marks, { categories = [], axisType = "category", sideways = false, bars = false, theme, locale }) {
  const given = marksFrom(marks);
  if (given.length === 0) return { series: [], extent: null };
  const colour = (tone) => theme[tone] ?? theme.muted;
  // In a BarChart the value axis is x.
  const valueKey = sideways ? "xAxis" : "yAxis";
  const atKey = sideways ? "yAxis" : "xAxis";
  const lines = [];
  const areas = [];
  for (const mark of given) {
    const ends = mark.on === "value" ? [mark.from, mark.to] : atCoords(mark, { categories, axisType, bars, locale });
    if (!ends) continue;
    const key = mark.on === "value" ? valueKey : atKey;
    const color = colour(mark.tone);
    const label = mark.label
      ? { show: true, formatter: () => mark.label, color, fontFamily: theme.font, fontSize: 11 }
      : { show: false };
    if (mark.band) {
      areas.push([
        {
          [key]: ends[0],
          itemStyle: { color: withAlpha(color, 0.12) },
          label: { ...label, position: mark.on === "value" && sideways ? "insideTopLeft" : "insideTop" },
        },
        { [key]: ends[1] },
      ]);
    } else {
      lines.push({
        [key]: ends[0],
        lineStyle: { color, type: "dashed", width: 1 },
        label: { ...label, position: "insideEndTop" },
      });
    }
  }
  const quiet = { type: "line", data: [], silent: true, legendHoverLink: false, tooltip: { show: false }, animation: false };
  const series = [];
  if (areas.length) series.push({ ...quiet, id: MARK_AREAS, z: 1, markArea: { silent: true, data: areas } });
  if (lines.length) series.push({ ...quiet, id: MARK_LINES, z: 3, markLine: { silent: true, symbol: "none", data: lines } });
  const values = given.filter((m) => m.on === "value").flatMap((m) => [m.from, m.to]);
  return { series, extent: values.length ? [Math.min(...values), Math.max(...values)] : null };
}

/**
 * The axis options that take in the marks' extent: ECharts widens the data's
 * own min and max to it, and rounds the axis as it would the data's.
 */
export function widened(axis, extent) {
  if (!extent) return axis;
  return { ...axis, dataMin: extent[0], dataMax: extent[1] };
}

/** Where a value lies along the axis the window is on, to compare with the window's ends. */
function positionOf(axis, value, locale) {
  if (axis.type === "category") return nearestCategory(axis.values, value, locale);
  if (axis.type === "time") return msOf(value);
  return Number(value);
}

/**
 * What the marks say, a sentence each (RFC 0065): a value line with the
 * points over it, a value band with the points in it (with one series or a
 * stack, whose totals are counted), a line at a category, a period.
 *
 * `shown(i)` says whether the `i`th category (or a scatter's point by its x)
 * is in the window; `axis` and `range` are the window's, null without one,
 * and an `at` mark outside it isn't said: null in its place, so the
 * sentences stay aligned with the marks (`bandRows`).
 */
export function marksSaid(marks, { kind, series, categories, stack = false, percent = false, format, locale, words, axis = null, range = null, shown = () => true }) {
  const given = marksFrom(marks);
  if (given.length === 0) return [];
  const say = formatter(format, locale);
  const scatter = kind === "scatter";
  // The numbers a value mark is measured against: one series' points, or a stack's totals.
  let values = null;
  if (!percent) {
    if (scatter && series.length === 1) {
      values = series[0].data.filter((p) => shown(Array.isArray(p) ? p[0] : p?.value?.[0])).map(valueOf);
    } else if (!scatter && (series.length === 1 || stack)) {
      const count = Math.max(categories.length, ...series.map((s) => s.data.length));
      values = Array.from({ length: count }, (_, i) => i)
        .filter((i) => shown(i))
        .map((i) => {
          const each = series.map((s) => valueOf(s.data[i]));
          return each.every((v) => v === null) ? null : each.reduce((sum, v) => sum + (v ?? 0), 0);
        });
    }
    values = values?.filter((v) => v !== null) ?? null;
  }
  const atText = (v) => {
    if (scatter) return say(Number(v));
    const i = nearestCategory(categories, v, locale);
    return categoryText(i >= 0 ? categories[i] : v, locale);
  };
  const inWindow = (mark) => {
    if (!axis || !range) return true;
    const [lo, hi] = range.map((v) => positionOf(axis, v, locale));
    const [a, b] = [positionOf(axis, mark.from, locale), positionOf(axis, mark.to, locale)];
    if (![lo, hi, a, b].every(Number.isFinite)) return true;
    return Math.max(a, b) >= lo && Math.min(a, b) <= hi;
  };
  return given.map((mark) => {
    const name = mark.label ?? words.mark;
    if (mark.on === "at") {
      if (!inWindow(mark)) return null;
      return mark.band ? words.markSpan(name, atText(mark.from), atText(mark.to)) : words.markAt(name, atText(mark.from));
    }
    if (!mark.band) {
      if (!values?.length) return words.markValue(name, say(mark.from));
      return words.markOver(name, say(mark.from), values.filter((v) => v > mark.from).length, values.length);
    }
    if (!values?.length) return words.markBand(name, say(mark.from), say(mark.to));
    const inside = values.filter((v) => v >= mark.from && v <= mark.to).length;
    return words.markIn(name, say(mark.from), say(mark.to), inside, values.length);
  });
}

/**
 * The categories each `at` band covers (RFC 0065), by index: the hidden
 * table's rows point at the band's sentence. `[[markIndex, Set], …]`, by
 * the mark's place in `marksSaid`.
 */
export function bandRows(marks, { categories, locale }) {
  return marksFrom(marks).flatMap((mark, i) => {
    if (mark.on !== "at" || !mark.band) return [];
    const a = nearestCategory(categories, mark.from, locale);
    const b = nearestCategory(categories, mark.to, locale);
    if (a < 0 || b < 0) return [];
    const [lo, hi] = a <= b ? [a, b] : [b, a];
    return [[i, new Set(Array.from({ length: hi - lo + 1 }, (_, k) => lo + k))]];
  });
}
