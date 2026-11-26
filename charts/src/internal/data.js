// WireCharts' data contract (RFC 0058): `series` is `[{ name, data, …extra }]`,
// a bare list of numbers is one series, and `categories` is the x axis. A
// live query's value is the series, or `{ series, categories }`.

/** A series list from what was given: `[{ name, data }]`, `[1, 2, 3]`, `[[x, y], …]`. */
export function normalizeSeries(series) {
  if (!Array.isArray(series) || series.length === 0) return [];
  const first = series[0];
  if (first !== null && typeof first === "object" && !Array.isArray(first)) {
    // `{ name, value }` points: one series (a pie's).
    if ("value" in first && !("data" in first)) return [{ data: series }];
    return series.map((s) => ({ ...s, data: s.data ?? [] }));
  }
  return [{ data: series }];
}

/** `{ series, categories }` from props and a live value, the live one winning. */
export function chartData({ series, categories }, value) {
  if (value === undefined || value === null) return { series: normalizeSeries(series), categories: categories ?? [] };
  if (Array.isArray(value)) return { series: normalizeSeries(value), categories: categories ?? [] };
  return { series: normalizeSeries(value.series ?? []), categories: value.categories ?? categories ?? [] };
}

/** A point's number: `12`, `{ value: 12 }`, `[x, 12]` (y), or `null` for a gap. */
export function valueOf(point) {
  if (typeof point === "number") return Number.isFinite(point) ? point : null;
  if (Array.isArray(point)) return typeof point[1] === "number" ? point[1] : null;
  if (point && typeof point === "object" && typeof point.value === "number") return point.value;
  return null;
}

/** Whether there's anything to draw. */
export function hasPoints(series) {
  return series.some((s) => s.data.some((p) => valueOf(p) !== null));
}

/** Categories that are all ISO dates are a time axis. */
export function isTime(categories) {
  return categories.length > 0 && categories.every((c) => typeof c === "string" && /^\d{4}-\d{2}-\d{2}/.test(c));
}

/** A number as text in `locale`, or by the chart's `format`. */
export function formatter(format, locale) {
  if (format) return format;
  const number = new Intl.NumberFormat(locale, { maximumFractionDigits: 2 });
  return (value) => (typeof value === "number" ? number.format(value) : value === null || value === undefined ? "—" : String(value));
}

/** A category as text: an ISO date in `locale`'s words, else as given. */
export function categoryText(category, locale) {
  if (typeof category === "string" && /^\d{4}-\d{2}-\d{2}$/.test(category)) {
    return new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone: "UTC" }).format(new Date(`${category}T00:00:00Z`));
  }
  return String(category ?? "");
}

/**
 * The rows of the chart's data as a table: a pie's slices, else one row per
 * category (or per point, for a scatter) and a column per series.
 */
export function tableOf(kind, series, categories, format, locale) {
  const say = formatter(format, locale);
  if (kind === "pie") {
    const slices = series[0]?.data ?? [];
    return {
      columns: [series[0]?.name ?? ""],
      rows: slices.map((p, i) => [p?.name ?? categoryText(categories[i], locale), [say(valueOf(p))]]),
    };
  }
  if (kind === "scatter") {
    return {
      columns: series.map((s, i) => s.name ?? `${i + 1}`),
      rows: series.flatMap((s, i) =>
        s.data.map((p) => [s.name ?? `${i + 1}`, series.map((_, j) => (j === i ? `${say(p?.[0])}, ${say(p?.[1])}` : ""))]),
      ),
    };
  }
  const count = Math.max(categories.length, ...series.map((s) => s.data.length));
  return {
    columns: series.map((s, i) => s.name ?? `${i + 1}`),
    rows: Array.from({ length: count }, (_, row) => [
      categoryText(categories[row] ?? row + 1, locale),
      series.map((s) => say(valueOf(s.data[row]))),
    ]),
  };
}

/** What the chart says to a screen reader, in its words. */
export function summaryOf(kind, series, categories, format, locale, words) {
  const say = formatter(format, locale);
  if (!hasPoints(series)) return words.empty;
  if (kind === "pie") {
    const slices = (series[0]?.data ?? [])
      .map((p, i) => ({ name: p?.name ?? categoryText(categories[i], locale), value: valueOf(p) ?? 0 }))
      .sort((a, b) => b.value - a.value);
    const total = slices.reduce((sum, s) => sum + s.value, 0) || 1;
    const percent = new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 0 });
    return slices
      .slice(0, 3)
      .map((s) => words.slice(s.name, percent.format(s.value / total)))
      .join(", ");
  }
  return series
    .map((s, i) => {
      const values = s.data.map(valueOf).filter((v) => v !== null);
      if (values.length === 0) return null;
      const name = s.name ?? `${i + 1}`;
      return words.series(name, say(values[0]), say(values[values.length - 1]), say(Math.min(...values)), say(Math.max(...values)), values.length);
    })
    .filter(Boolean)
    .join(". ");
}
