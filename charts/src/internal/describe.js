// What a WireCharts chart says to a screen reader and the assistant (RFC
// 0059), read from the option it draws: a summary, and its data as a table.
// Each family reads its own shape: slices, flows, cells, a hierarchy, OHLC
// rows, a radar's indicators, or categories by series.

import { categoryText, formatter } from "./data.js";

const list = (value) => (Array.isArray(value) ? value : value ? [value] : []);

/** A point's number: `12`, `{ value: 12 }`, `[x, 12]` (the last), else null. */
function numberOf(point) {
  const v = point && typeof point === "object" && !Array.isArray(point) ? point.value : point;
  if (typeof v === "number") return Number.isFinite(v) ? v : null;
  if (Array.isArray(v)) return typeof v[v.length - 1] === "number" ? v[v.length - 1] : null;
  return null;
}

/** The axis that holds categories, if the chart has one. */
function categoryAxis(option) {
  for (const axis of [...list(option.xAxis), ...list(option.yAxis), ...list(option.singleAxis)]) {
    if (axis?.type === "category" && Array.isArray(axis.data)) return axis.data.map((c) => (c && typeof c === "object" ? c.value : c));
  }
  return null;
}

function slices(series, ctx) {
  const say = formatter(ctx.format, ctx.locale);
  const invisible = (p) => p?.itemStyle && (p.itemStyle.color === "none" || p.itemStyle.color === "transparent" || p.itemStyle.opacity === 0);
  const data = list(series.data)
    .filter((p) => !invisible(p))
    .map((p, i) => ({ name: p?.name ?? String(i + 1), value: numberOf(p) ?? 0 }));
  const total = data.reduce((sum, s) => sum + Math.abs(s.value), 0) || 1;
  const percent = new Intl.NumberFormat(ctx.locale, { style: "percent", maximumFractionDigits: 0 });
  const largest = [...data].sort((a, b) => b.value - a.value).slice(0, 3);
  return {
    empty: data.length === 0,
    summary: largest.map((s) => ctx.words.slice(s.name, percent.format(s.value / total))).join(", "),
    table: { head: ctx.words.name, columns: [series.name ?? ctx.words.value], rows: data.map((s) => [s.name, [say(s.value)]]) },
    count: data.length,
    select: (at) => data[at] && { series: series.name, category: data[at].name, value: data[at].value },
  };
}

function gauge(series, ctx) {
  const say = formatter(ctx.format, ctx.locale);
  const rows = list(series).flatMap((s) => list(s.data).map((p) => [p?.name || s.name || "", [say(numberOf(p))]]));
  return {
    empty: rows.length === 0,
    summary: rows.map(([name, [value]]) => ctx.words.value(name, value, say(series[0]?.max ?? 100))).join(", "),
    table: { head: ctx.words.name, columns: [ctx.words.valueHead], rows },
    count: 0,
    select: () => null,
  };
}

function flows(series, ctx) {
  const say = formatter(ctx.format, ctx.locale);
  const nodes = list(series.data ?? series.nodes);
  const name = (n) => (typeof n === "number" ? (nodes[n]?.name ?? String(n)) : String(n));
  const links = list(series.links ?? series.edges).map((l) => ({ from: name(l.source), to: name(l.target), value: numberOf(l) }));
  if (links.length === 0) {
    // A packed bubble: nodes only.
    return slices({ name: series.name, data: nodes.map((n) => ({ name: n.name, value: numberOf(n) ?? n.symbolSize ?? 0 })) }, ctx);
  }
  const largest = [...links].sort((a, b) => (b.value ?? 0) - (a.value ?? 0)).slice(0, 3);
  return {
    empty: false,
    summary: largest.map((l) => (l.value === null ? ctx.words.link(l.from, l.to) : ctx.words.flow(l.from, l.to, say(l.value)))).join(", "),
    table: {
      head: ctx.words.source,
      columns: [ctx.words.target, ctx.words.valueHead],
      rows: links.map((l) => [l.from, [l.to, l.value === null ? "" : say(l.value)]]),
    },
    count: links.length,
    select: (at) => links[at] && { series: series.name, category: `${links[at].from} → ${links[at].to}`, value: links[at].value },
  };
}

function hierarchy(series, ctx) {
  const say = formatter(ctx.format, ctx.locale);
  const rows = [];
  let depth = 0;
  const walk = (nodes, path, level) => {
    depth = Math.max(depth, level);
    for (const node of list(nodes)) {
      const here = [...path, node.name ?? ""];
      if (node.children?.length) walk(node.children, here, level + 1);
      else rows.push([here.join(" › "), [say(numberOf(node))]]);
    }
  };
  walk(series.data, [], 1);
  return {
    empty: rows.length === 0,
    summary: `${ctx.words.items(rows.length)}, ${ctx.words.levels(depth)}`,
    table: { head: ctx.words.name, columns: [ctx.words.valueHead], rows },
    count: 0,
    select: () => null,
  };
}

function cells(option, series, ctx) {
  const say = formatter(ctx.format, ctx.locale);
  const xs = list(option.xAxis)[0]?.data ?? [];
  const ys = list(option.yAxis)[0]?.data ?? [];
  const data = list(series.data).map((p) => (Array.isArray(p) ? p : p?.value)).filter(Array.isArray);
  const rows = data.map(([x, y, v]) => [`${xs[x] ?? x}, ${ys[y] ?? y}`, [say(v)]]);
  const top = data.reduce((best, c) => (best === null || c[2] > best[2] ? c : best), null);
  return {
    empty: rows.length === 0,
    summary: top ? ctx.words.highest(`${xs[top[0]] ?? top[0]}, ${ys[top[1]] ?? top[1]}`, say(top[2])) : ctx.words.empty,
    table: { columns: [ctx.words.valueHead], rows },
    count: 0,
    select: () => null,
  };
}

/** A streamgraph's `[when, value, name]` points, as categories by series. */
function river(series, ctx) {
  const points = list(series.data).filter(Array.isArray);
  const whens = [...new Set(points.map((p) => p[0]))];
  const names = [...new Set(points.map((p) => p[2]))];
  const shaped = names.map((name) => ({
    name,
    data: whens.map((w) => points.find((p) => p[0] === w && p[2] === name)?.[1] ?? null),
  }));
  return cartesian({ xAxis: { type: "category", data: whens } }, shaped, ctx);
}

function radar(option, series, ctx) {
  const say = formatter(ctx.format, ctx.locale);
  const indicators = list(list(option.radar)[0]?.indicator).map((i) => i.name);
  const entries = list(series.data);
  const rows = indicators.map((name, i) => [name, entries.map((e) => say(list(e.value)[i]))]);
  return {
    empty: entries.length === 0,
    summary: entries.map((e, i) => e.name ?? `${i + 1}`).join(", "),
    table: { head: ctx.words.category, columns: entries.map((e, i) => e.name ?? `${i + 1}`), rows },
    count: 0,
    select: () => null,
  };
}

/** Rows of several values per category: a candlestick's OHLC, a boxplot's five numbers. */
function boxes(option, series, ctx, heads) {
  const say = formatter(ctx.format, ctx.locale);
  const cats = categoryAxis(option) ?? [];
  const data = list(series.data).map((p) => (Array.isArray(p) ? p : list(p?.value)));
  const rows = data.map((values, i) => [categoryText(cats[i] ?? i + 1, ctx.locale), values.map(say)]);
  return {
    empty: rows.length === 0,
    summary: rows.length ? `${rows[0][0]} – ${rows[rows.length - 1][0]}: ${rows.length}` : ctx.words.empty,
    table: { columns: heads.slice(0, data[0]?.length ?? heads.length), rows },
    count: rows.length,
    select: (at) => rows[at] && { series: series.name, category: rows[at][0], value: data[at] },
  };
}

/**
 * A series drawn only to place another (a range's invisible base, a fan's
 * lower edge): no line, no fill, or no mark.
 */
function hidden(s) {
  const none = (style) => style && (style.opacity === 0 || style.color === "transparent" || style.color === "none");
  return none(s.lineStyle) && (!s.areaStyle || none(s.areaStyle)) || (none(s.itemStyle) && s.type === "bar");
}

/** Categories by series: lines, areas, bars, scatters and their combinations. */
function cartesian(option, all, ctx) {
  const say = formatter(ctx.format, ctx.locale);
  const cats = categoryAxis(option);
  const series = all.filter((s) => !hidden(s));
  const named = series.filter((s) => s.name !== undefined && s.name !== null && s.name !== "");
  // Unnamed series drawing the same values (a lollipop's stick and head): the first.
  const shown = named.length ? named : series.slice(0, 1);
  const columns = shown.map((s, i) => s.name || (shown.length === 1 ? ctx.words.valueHead : `${i + 1}`));
  const count = cats ? cats.length : Math.max(0, ...shown.map((s) => list(s.data).length));
  const label = (i) => {
    if (cats) return categoryText(cats[i], ctx.locale);
    const p = list(shown[0]?.data)[i];
    const x = Array.isArray(p) ? p[0] : Array.isArray(p?.value) ? p.value[0] : i + 1;
    return categoryText(typeof x === "number" ? say(x) : x, ctx.locale);
  };
  const rows = Array.from({ length: count }, (_, i) => [label(i), shown.map((s) => say(numberOf(list(s.data)[i])))]);
  const summary = shown
    .map((s, i) => {
      const values = list(s.data).map(numberOf).filter((v) => v !== null);
      if (values.length === 0) return null;
      return ctx.words.series(
        columns[i],
        say(values[0]),
        say(values[values.length - 1]),
        say(Math.min(...values)),
        say(Math.max(...values)),
        values.length,
      );
    })
    .filter(Boolean)
    .join(". ");
  return {
    empty: rows.length === 0 || shown.every((s) => list(s.data).every((p) => numberOf(p) === null)),
    summary: summary || ctx.words.empty,
    table: { columns, rows },
    count,
    select: (at) => {
      const s = shown[0];
      return s && { series: s.name, category: label(at), value: numberOf(list(s.data)[at]) };
    },
  };
}

/** What a chart's option says, by the kind of its first series. */
export function describeOption(option, ctx) {
  const series = list(option?.series);
  const first = series[0] ?? {};
  switch (first.type) {
    case "pie":
    case "funnel":
      return slices(first, ctx);
    case "gauge":
      return gauge(series, ctx);
    case "sankey":
    case "graph":
      return flows(first, ctx);
    case "tree":
    case "treemap":
    case "sunburst":
      return hierarchy(first, ctx);
    case "heatmap":
      return cells(option, first, ctx);
    case "themeRiver":
      return river(first, ctx);
    case "radar":
      return radar(option, first, ctx);
    case "candlestick":
      return boxes(option, first, ctx, [ctx.words.open, ctx.words.close, ctx.words.low, ctx.words.high]);
    case "boxplot":
      return boxes(option, first, ctx, [ctx.words.min, ctx.words.q1, ctx.words.median, ctx.words.q3, ctx.words.max]);
    default:
      return cartesian(option, series, ctx);
  }
}
