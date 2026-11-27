// A selection (RFC 0063): ECharts' brush in the tokens, and what it selects
// as the app's data, not ECharts' indexes: a scatter's points, or a span of
// categories. Plain functions over the option, as zoom.js's are.

const list = (v) => (Array.isArray(v) ? v : v === undefined || v === null ? [] : [v]);
const msOf = (v) => (typeof v === "number" ? v : v instanceof Date ? v.getTime() : Date.parse(v));
const pointOf = (p) => (Array.isArray(p) ? p : Array.isArray(p?.value) ? p.value : null);

/**
 * `color` at `alpha`: the brush is drawn over the points, so its fill lets
 * them through (`--elyra-accent-soft` is a solid colour in the themes).
 */
export function withAlpha(color, alpha) {
  const text = String(color).trim();
  const hex = text.match(/^#([0-9a-f]{3}|[0-9a-f]{6})$/i);
  if (hex) {
    const h = hex[1].length === 3 ? [...hex[1]].map((c) => c + c).join("") : hex[1];
    const [r, g, b] = [0, 2, 4].map((i) => parseInt(h.slice(i, i + 2), 16));
    return `rgba(${r}, ${g}, ${b}, ${alpha})`;
  }
  const rgb = text.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i);
  return rgb ? `rgba(${rgb[1]}, ${rgb[2]}, ${rgb[3]}, ${alpha})` : text;
}

/** The brush's shape: a rectangle over a scatter (a lasso with Shift), a span along the category axis. */
export function brushTypeOf(scatter, axis, lasso = false) {
  if (scatter) return lasso ? "polygon" : "rect";
  return axis?.vertical ? "lineY" : "lineX";
}

/** The option with ECharts' brush, in the tokens' colours, its toolbox hidden. */
export function withBrush(option, theme, scatter, axis) {
  const along = scatter ? { xAxisIndex: 0, yAxisIndex: 0 } : axis?.vertical ? { yAxisIndex: 0 } : { xAxisIndex: 0 };
  return {
    ...option,
    brush: {
      toolbox: [],
      ...along,
      brushMode: "single",
      transformable: true,
      throttleType: "debounce",
      throttleDelay: 80,
      brushStyle: { borderWidth: 1, color: withAlpha(theme.accent, 0.15), borderColor: theme.accent },
      outOfBrush: { colorAlpha: 0.35 },
    },
    // A drag brushes; the wheel still zooms (RFC 0061).
    ...(option.dataZoom
      ? { dataZoom: list(option.dataZoom).map((dz) => (dz.type === "inside" ? { ...dz, moveOnMouseMove: false } : dz)) }
      : {}),
    // Points chosen by the keys are marked as selected.
    ...(scatter
      ? {
          series: list(option.series).map((s) => ({
            ...s,
            selectedMode: "multiple",
            select: { itemStyle: { borderColor: theme.accent, borderWidth: 2 } },
          })),
        }
      : {}),
  };
}

/** A scatter's point as the app gets it. */
export function pointAt(series, seriesIndex, index) {
  const s = series[seriesIndex];
  const p = pointOf(list(s?.data)[index]);
  if (!p) return null;
  return {
    series: s.name ?? `${seriesIndex + 1}`,
    x: p[0],
    y: p[1],
    ...(p.length > 2 ? { size: p[2] } : {}),
    seriesIndex,
    index,
  };
}

/**
 * What a `brushSelected` says, as `selection`: `{ points }` for a scatter,
 * `{ from, to, categories }` along an axis (the first series' points under
 * the brush), or null when nothing is under it.
 */
export function selectionOf(params, option, axis, scatter) {
  const selected = list(params?.batch?.[0]?.selected);
  const series = list(option?.series);
  if (scatter) {
    const points = selected.flatMap((s) => list(s.dataIndex).map((i) => pointAt(series, s.seriesIndex, i))).filter(Boolean);
    return points.length ? { points } : null;
  }
  const first = selected.find((s) => list(s.dataIndex).length > 0);
  if (!first || !axis) return null;
  const categories = [...list(first.dataIndex)].sort((a, b) => a - b).map((i) => axis.values[i]).filter((v) => v !== undefined);
  if (categories.length === 0) return null;
  return { from: categories[0], to: categories[categories.length - 1], categories };
}

/** The span from the `a`th category to the `b`th, as a selection. */
export function spanOf(axis, a, b) {
  const [i, j] = a <= b ? [a, b] : [b, a];
  const categories = axis.values.slice(i, j + 1);
  return categories.length ? { from: categories[0], to: categories[categories.length - 1], categories } : null;
}

/** Where to draw a selection the app set: the span, or the rectangle around the points. */
export function areasOf(selection, axis, scatter) {
  if (!selection) return [];
  if (scatter) {
    const points = list(selection.points);
    if (points.length === 0) return [];
    const xs = points.map((p) => p.x);
    const ys = points.map((p) => p.y);
    return [
      {
        brushType: "rect",
        xAxisIndex: 0,
        yAxisIndex: 0,
        coordRange: [
          [Math.min(...xs), Math.max(...xs)],
          [Math.min(...ys), Math.max(...ys)],
        ],
      },
    ];
  }
  if (!axis) return [];
  const along = axis.vertical ? { brushType: "lineY", yAxisIndex: 0 } : { brushType: "lineX", xAxisIndex: 0 };
  if (axis.type === "category") {
    const i = axis.values.findIndex((v) => String(v) === String(selection.from));
    const j = axis.values.findIndex((v) => String(v) === String(selection.to));
    return i < 0 || j < 0 ? [] : [{ ...along, coordRange: [Math.min(i, j), Math.max(i, j)] }];
  }
  const at = axis.type === "time" ? msOf : Number;
  return [{ ...along, coordRange: [at(selection.from), at(selection.to)] }];
}

/** Two selections the same. */
export function sameSelection(a, b) {
  if (a === b) return true;
  if (!a || !b) return false;
  if (a.points || b.points) {
    const keys = (s) => list(s.points).map((p) => `${p.seriesIndex ?? p.series}:${p.index ?? `${p.x},${p.y}`}`).sort().join("|");
    return keys(a) === keys(b);
  }
  return String(a.from) === String(b.from) && String(a.to) === String(b.to);
}

/**
 * Whether the table's `r`th row is selected: a category's row on an axis, a
 * point's row in a scatter's table (its points, series by series).
 */
export function selectedBy(selection, axis, series, scatter) {
  if (!selection) return () => false;
  if (scatter) {
    const keys = new Set(list(selection.points).map((p) => `${p.seriesIndex}:${p.index}`));
    const rows = series.flatMap((s, si) => list(s.data).map((_, i) => `${si}:${i}`));
    return (r) => keys.has(rows[r]);
  }
  const chosen = new Set(list(selection.categories).map(String));
  return (r) => axis !== null && axis !== undefined && chosen.has(String(axis.values[r]));
}

/** How many points a scatter has, for "12 of 340". */
export const pointCount = (series) => series.reduce((n, s) => n + list(s.data).length, 0);
