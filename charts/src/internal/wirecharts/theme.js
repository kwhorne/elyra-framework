// The app's look over a WireCharts option (RFC 0059): WireCharts leaves the
// colours to an ECharts theme; here they're the design tokens, set where
// the option hasn't set its own (a monochrome pie, a fan's colour keep theirs).

import { themed } from "../theme.js";

const list = (value) => (Array.isArray(value) ? value : value ? [value] : []);
const each = (value, fn) => (Array.isArray(value) ? value.map(fn) : value ? fn(value) : value);
const isMap = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
/** The series whose labels sit beside their marks, on the chart's surface. */
const LABELLED = new Set(["pie", "line", "tree", "graph", "scatter", "radar", "gauge"]);

/** `style` under `own`: what the option set wins. */
function under(style, own) {
  if (!isMap(own)) return own === undefined ? style : own;
  const out = { ...style };
  for (const [key, value] of Object.entries(own)) out[key] = isMap(value) && isMap(style[key]) ? under(style[key], value) : value;
  return out;
}

/**
 * ECharts 6 can't colour a line by pieces open at both ends (`lte: 0`,
 * `gt: 0`: no stops, and it throws), which ECharts 5, WireCharts', could.
 * Each open end is closed far away, which draws the same.
 */
function bounded(visualMap) {
  if (!isMap(visualMap) || !Array.isArray(visualMap.pieces)) return visualMap;
  const far = 1e15;
  const pieces = visualMap.pieces.map((p) => {
    if (!isMap(p) || "value" in p || "min" in p || "max" in p) return p;
    const below = "lt" in p || "lte" in p;
    const above = "gt" in p || "gte" in p;
    if (below && !above) return { ...p, gte: -far };
    if (above && !below) return { ...p, lte: far };
    return p;
  });
  return { ...visualMap, pieces };
}

/** A WireCharts option in the app's look. */
export function applyTheme(option, theme) {
  const t = themed(theme);
  const axis = (a) => (isMap(a) ? under(t.axis, a) : a);
  const out = {
    ...option,
    color: option.color ?? t.color,
    backgroundColor: "transparent",
    textStyle: under(t.textStyle, option.textStyle),
    animation: theme.duration > 0 ? (option.animation ?? true) : false,
    ...(theme.duration > 0 && option.animationDuration === undefined ? { animationDuration: t.animationDuration } : {}),
    aria: { enabled: true, decal: { show: false } },
  };
  if (option.tooltip !== undefined && !Array.isArray(option.tooltip)) out.tooltip = under(t.tooltip, option.tooltip);
  else if (Array.isArray(option.tooltip) && option.tooltip.length === 0) out.tooltip = { ...t.tooltip };
  if (option.legend !== undefined) out.legend = each(option.legend, (l) => under(t.legend, l));
  for (const key of ["xAxis", "yAxis", "singleAxis", "radiusAxis", "angleAxis"]) {
    if (option[key] !== undefined) out[key] = each(option[key], axis);
  }
  if (option.radar !== undefined) {
    out.radar = each(option.radar, (r) =>
      under({ axisName: { color: theme.muted }, splitLine: { lineStyle: { color: theme.border } }, axisLine: { lineStyle: { color: theme.border } } }, r),
    );
  }
  if (option.title !== undefined) out.title = each(option.title, (x) => under({ textStyle: { color: theme.text } }, x));
  if (option.visualMap !== undefined) {
    out.visualMap = each(option.visualMap, (v) => bounded(under({ textStyle: { color: theme.muted } }, v)));
  }
  const categories = list(option.singleAxis)[0]?.type === "category" ? list(option.singleAxis)[0].data : null;
  out.series = list(option.series).map((s) => {
    if (!isMap(s)) return s;
    let series = s;
    // A pie's slices are edged in the surface they sit on.
    if (series.type === "pie" && !series.itemStyle?.borderColor) {
      series = under({ itemStyle: { borderColor: t.tooltip.backgroundColor } }, series);
    }
    // Labels beside the marks are the theme's text, without ECharts' outline,
    // which reads as a smudge on a dark surface.
    if (LABELLED.has(series.type)) series = under({ label: { color: theme.text, textBorderWidth: 0 }, endLabel: { color: theme.text, textBorderWidth: 0 } }, series);
    // A gauge's numbers, value and name, and its ticks.
    if (series.type === "gauge") {
      series = under(
        {
          axisLabel: { color: theme.muted },
          detail: { color: theme.text },
          title: { color: theme.muted },
          axisTick: { lineStyle: { color: theme.muted } },
          splitLine: { lineStyle: { color: theme.muted } },
          // The track the value runs along, unless the chart has its bands.
          axisLine: { lineStyle: { color: [[1, theme.border]] } },
        },
        series,
      );
    }
    // A streamgraph's times on a category axis: ECharts 6 reads "2019" as the
    // number 2019, not the category, and draws the river far off the chart.
    if (series.type === "themeRiver" && categories && Array.isArray(series.data)) {
      series = {
        ...series,
        data: series.data.map((p) => (Array.isArray(p) && categories.includes(p[0]) ? [categories.indexOf(p[0]), ...p.slice(1)] : p)),
      };
    }
    return series;
  });
  return out;
}
