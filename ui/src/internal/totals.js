// A table's totals (RFC 0067): a column's `total` over rows, as plain
// functions a test reads without a DOM. `"sum"`, `"avg"`, `"min"`, `"max"`,
// `"count"`, or the app's own `(rows, column) => value`.

const KINDS = ["sum", "avg", "min", "max", "count"];

/** Whether a column has a total. */
export const hasTotal = (column) => KINDS.includes(column?.total) || typeof column?.total === "function";

/** The decimals a number is written with: `0.25` has 2. */
function decimalsOf(n) {
  const text = String(n);
  if (/e/i.test(text)) return 15;
  const dot = text.indexOf(".");
  return dot < 0 ? 0 : text.length - dot - 1;
}

/** `n` to `places` decimals, so `0.1 + 0.2` is `0.3`. */
const round = (n, places) => Number(n.toFixed(Math.min(15, places)));

/** One column's total over `rows`; null when there's nothing to total. */
export function totalOf(rows, column) {
  if (typeof column.total === "function") return column.total(rows, column);
  const values = rows.map((row) => row?.[column.key]);
  if (column.total === "count") return values.filter((v) => v !== null && v !== undefined && v !== "").length;
  const numbers = values.filter((v) => typeof v === "number" && Number.isFinite(v));
  if (column.total === "sum") return round(numbers.reduce((a, b) => a + b, 0), Math.max(0, ...numbers.map(decimalsOf)));
  if (numbers.length === 0) return null;
  if (column.total === "min") return Math.min(...numbers);
  if (column.total === "max") return Math.max(...numbers);
  const sum = numbers.reduce((a, b) => a + b, 0);
  return round(sum / numbers.length, Math.max(0, ...numbers.map(decimalsOf)) + 2);
}

/** Each column's total by its key, for the columns that have one. */
export function totalsOf(rows, columns) {
  const out = {};
  for (const column of columns) if (hasTotal(column)) out[column.key] = totalOf(rows, column);
  return out;
}

/**
 * A total as the table shows it: by the column's `format` (given the total
 * and a row that has only it), except a count, which is a number of rows.
 */
export function totalText(value, column, locale) {
  if (value === null || value === undefined) return "";
  if (column.total !== "count" && column.format) return String(column.format(value, { [column.key]: value }) ?? "");
  if (typeof value === "number" && column.total === "count") return new Intl.NumberFormat(locale).format(value);
  return String(value);
}
