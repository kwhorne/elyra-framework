// A table's filters (RFC 0069): a column's own, by kind; matching rows and
// what a chip says. Plain functions, held by tests without a DOM.
//
//   filter: "text"                 → query.filters[key] = "ada"
//   filter: { options: [[v, l]] }  → ["open", "overdue"]
//   filter: "options"              → the same, the options from `rows`
//   filter: "range"                → { min?, max? }
//   filter: "dates"                → { from?, to? } (ISO days)

/** A column's filter kind: "text", "options", "range" or "dates"; null for none. */
export function kindOf(column) {
  const f = column?.filter;
  if (f === "text" || f === "range" || f === "dates" || f === "options") return f;
  if (f && typeof f === "object" && Array.isArray(f.options)) return "options";
  return null;
}

/**
 * A column's options as `[value, label]`: its own, or (for `rows`) the
 * values the rows have, each once, shown as the column shows them.
 */
export function optionsOf(column, rows = []) {
  const own = column?.filter?.options;
  if (Array.isArray(own)) return own.map((o) => (Array.isArray(o) ? [o[0], String(o[1] ?? o[0])] : [o, String(o)]));
  const seen = new Map();
  for (const row of rows) {
    const value = row?.[column.key];
    if (value === null || value === undefined || value === "" || seen.has(value)) continue;
    seen.set(value, column.format ? String(column.format(value, row) ?? value) : String(value));
  }
  return [...seen].sort((a, b) => a[1].localeCompare(b[1]));
}

/** Whether a filter's value filters nothing. */
export function isEmpty(kind, value) {
  if (value === null || value === undefined) return true;
  if (kind === "text") return String(value).trim() === "";
  if (kind === "options") return !Array.isArray(value) || value.length === 0;
  if (kind === "range") return !Number.isFinite(value.min) && !Number.isFinite(value.max);
  if (kind === "dates") return !value.from && !value.to;
  return true;
}

/** The filters with a value, for the columns that have a filter: empty ones left out. */
export function activeFilters(filters, columns) {
  const out = {};
  for (const column of columns) {
    const kind = kindOf(column);
    const value = filters?.[column.key];
    if (kind && !isEmpty(kind, value)) out[column.key] = value;
  }
  return out;
}

const day = (v) => (typeof v === "string" ? v.slice(0, 10) : v instanceof Date ? v.toISOString().slice(0, 10) : null);

/** Whether a row passes one filter. */
function passes(row, column, kind, value) {
  const cell = row?.[column.key];
  if (kind === "text") return String(cell ?? "").toLowerCase().includes(String(value).trim().toLowerCase());
  if (kind === "options") return value.some((v) => v === cell || String(v) === String(cell));
  if (kind === "range") {
    if (typeof cell !== "number" || !Number.isFinite(cell)) return false;
    return (!Number.isFinite(value.min) || cell >= value.min) && (!Number.isFinite(value.max) || cell <= value.max);
  }
  if (kind === "dates") {
    const d = day(cell);
    if (!d || !/^\d{4}-\d{2}-\d{2}$/.test(d)) return false;
    return (!value.from || d >= value.from) && (!value.to || d <= value.to);
  }
  return true;
}

/** The rows that pass every active filter. */
export function filterRows(rows, filters, columns) {
  const active = activeFilters(filters, columns);
  const checks = columns.filter((c) => c.key in active).map((c) => [c, kindOf(c), active[c.key]]);
  if (checks.length === 0) return rows;
  return rows.filter((row) => checks.every(([c, kind, value]) => passes(row, c, kind, value)));
}

/**
 * What a chip says of a filter: "Status: Open, Overdue", "Amount: 10,000 or
 * more", "Due: Nov 1, 2026 – Nov 30, 2026", "Due: From Nov 1, 2026",
 * "Customer: “ada”".
 */
export function chipText(column, value, { words, locale, rows = [] }) {
  const kind = kindOf(column);
  const number = new Intl.NumberFormat(locale);
  const date = (d) => new Intl.DateTimeFormat(locale, { dateStyle: "medium", timeZone: "UTC" }).format(new Date(`${d}T00:00:00Z`));
  let said = "";
  if (kind === "text") said = `“${String(value).trim()}”`;
  else if (kind === "options") {
    const labels = new Map(optionsOf(column, rows).map(([v, l]) => [String(v), l]));
    said = value.map((v) => labels.get(String(v)) ?? String(v)).join(", ");
  } else if (kind === "range") {
    const [min, max] = [value.min, value.max].map((n) => (Number.isFinite(n) ? number.format(n) : null));
    said = min && max ? words.between(min, max) : min ? words.atLeast(min) : words.atMost(max);
  } else if (kind === "dates") {
    const [from, to] = [value.from, value.to].map((d) => (d ? date(d) : null));
    said = from && to ? `${from} – ${to}` : from ? `${words.from} ${from}` : `${words.to} ${to}`;
  }
  return `${column.label}: ${said}`;
}
