// Targets (RFC 0072): a goal for each category, drawn as the bullet chart's
// tick across a column (or a dashed step line through a line's points) and
// said against the data: met or not, and by how much. Plain functions over
// the props, so a test reads them without a layout.

import { categoryText, formatter, valueOf } from "./data.js";

/** The series that draws the targets: never data. `brush.js` leaves it out by its id. */
export const TARGETS = "elyra-marks-targets";

/**
 * The app's `targets` as `{ label, data, tone, lower }`, `data` one number
 * (or null) a category; null without any.
 */
export function targetsFrom(targets, categories = []) {
  if (!targets || typeof targets !== "object" || !Array.isArray(targets.data)) return null;
  const count = Math.max(categories.length, 0);
  const data = Array.from({ length: count }, (_, i) => {
    const v = valueOf(targets.data[i]);
    return v === null ? null : v;
  });
  if (data.every((v) => v === null)) return null;
  return {
    label: targets.label == null || targets.label === "" ? null : String(targets.label),
    data,
    // Without a tone, the text's colour: a tick in a column's own colour is lost inside it.
    tone: ["muted", "accent", "success", "warning", "danger"].includes(targets.tone) ? targets.tone : null,
    lower: !!targets.lower,
  };
}

/**
 * What each category's target is measured against: the one series, or a
 * stack's total; null with several separate series, which have no one value.
 */
export function measuredOf(series, categories, stack = false) {
  if (series.length !== 1 && !stack) return null;
  return categories.map((_, i) => {
    const each = series.map((s) => valueOf(s.data[i]));
    return each.every((v) => v === null) ? null : each.reduce((sum, v) => sum + (v ?? 0), 0);
  });
}

/**
 * The series that draws the targets, and the extent the value axis takes
 * in. Over columns or bars, a `custom` series whose `renderItem` draws a
 * tick across each band, 70 % of its width; over a line or an area, a dashed
 * step line.
 */
export function targetsOf(targets, { categories = [], time = false, sideways = false, bars = false, theme }) {
  const t = targetsFrom(targets, categories);
  if (!t) return { series: [], extent: null, targets: null };
  const color = (t.tone && theme[t.tone]) || theme.text;
  const data = t.data.map((v, i) => (time ? [categories[i], v] : [i, v]));
  const name = t.label ?? "";
  const quiet = { id: TARGETS, name, silent: true, legendHoverLink: false, z: 4, animation: false };
  const series = bars
    ? {
        ...quiet,
        type: "custom",
        data: data.filter(([, v]) => v !== null),
        encode: sideways ? { x: 1, y: 0 } : { x: 0, y: 1 },
        renderItem: tickOf(color, sideways, time, theme.surface),
      }
    : {
        ...quiet,
        type: "line",
        data: time ? data : t.data,
        step: "middle",
        showSymbol: false,
        connectNulls: false,
        lineStyle: { color, type: "dashed", width: 2 },
        itemStyle: { color },
      };
  const values = t.data.filter((v) => v !== null);
  return { series: [series], extent: [Math.min(...values), Math.max(...values)], targets: t };
}

/**
 * A custom series' `renderItem`: a 3 px tick across the band at the target,
 * edged in the surface's colour so it reads inside a column and above it.
 */
export function tickOf(color, sideways, time, edge) {
  return (params, api) => {
    const category = api.value(0);
    const value = api.value(1);
    const at = api.coord(sideways ? [value, category] : [category, value]);
    // A band's width (a category's), or for a time axis a day's, at most 48 px.
    const band = api.size(sideways ? [0, 1] : [1, 0]);
    const across = Math.min(48, Math.max(8, (sideways ? band[1] : band[0]) * (time ? 1 : 0.7)));
    const shape = sideways
      ? { x: at[0] - 1.5, y: at[1] - across / 2, width: 3, height: across }
      : { x: at[0] - across / 2, y: at[1] - 1.5, width: across, height: 3 };
    return { type: "rect", shape, style: edge ? { fill: color, stroke: edge, lineWidth: 1 } : { fill: color } };
  };
}

/** Whether a value meets its target: at or over, or at or under when `lower`. */
export const meets = (value, target, lower) => (lower ? value <= target : value >= target);

/**
 * What the targets say in the summary: "Budget: 7 of 12 met", over what's
 * shown; "Budget shown" when there's no one value to measure.
 */
export function targetsSaid(targets, { series, categories, stack = false, words, shown = () => true }) {
  const t = targetsFrom(targets, categories);
  if (!t) return null;
  const name = t.label ?? words.target;
  const measured = measuredOf(series, categories, stack);
  if (!measured) return words.targetShown(name);
  let count = 0;
  let total = 0;
  t.data.forEach((target, i) => {
    if (target === null || measured[i] === null || !shown(i)) return;
    total++;
    if (meets(measured[i], target, t.lower)) count++;
  });
  return words.targetMet(name, count, total);
}

/**
 * The table's columns for the targets: the target and the difference from
 * what's measured, a row a category, in the chart's format.
 */
export function targetColumns(targets, { series, categories, stack = false, format, locale, words, difference = true }) {
  const t = targetsFrom(targets, categories);
  if (!t) return null;
  const say = formatter(format, locale);
  const measured = measuredOf(series, categories, stack);
  const signed = (n) => (n > 0 ? `+${say(n)}` : say(n));
  const rows = categories.map((_, i) => {
    const target = t.data[i];
    const cells = [target === null ? "" : say(target)];
    if (difference) cells.push(target === null || !measured || measured[i] === null ? "" : signed(measured[i] - target));
    return cells;
  });
  return { columns: difference ? [t.label ?? words.target, words.difference] : [t.label ?? words.target], rows };
}

/**
 * The tooltip at a category, with the target's difference: each series' line
 * as ECharts draws it, then "Difference −18,000".
 */
export function tooltipWithTargets(targets, { series, categories, stack = false, format, locale, words }) {
  const t = targetsFrom(targets, categories);
  const say = formatter(format, locale);
  const measured = t ? measuredOf(series, categories, stack) : null;
  return (params) => {
    const list = Array.isArray(params) ? params : [params];
    if (list.length === 0) return "";
    const head = list[0].axisValueLabel ?? categoryText(list[0].name, locale);
    const lines = list
      .map((p) => {
        const v = Array.isArray(p.value) ? p.value[1] : p.value;
        if (v === null || v === undefined || v === "-") return null;
        return `${p.marker ?? ""}${escape(p.seriesName ?? "")} <b>${escape(say(v))}</b>`;
      })
      .filter(Boolean);
    const i = list[0].dataIndex;
    if (t && measured && t.data[i] !== null && measured[i] !== null && measured[i] !== undefined) {
      const d = measured[i] - t.data[i];
      lines.push(`${escape(words.difference)} <b>${escape(d > 0 ? `+${say(d)}` : say(d))}</b>`);
    }
    return [escape(String(head)), ...lines].join("<br>");
  };
}

const escape = (text) => String(text).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
