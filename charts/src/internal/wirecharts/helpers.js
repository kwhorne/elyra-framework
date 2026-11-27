// What WireCharts' Option.php builders share (RFC 0059), ported as they are:
// each builder in this folder returns the option Option.php returns for the
// same arguments, with real functions where PHP writes "@@function …@@".

/** PHP's `$a ?? $b`: `$b` when `$a` is unset or null. */
export const or = (value, fallback) => (value === undefined || value === null ? fallback : value);

/** PHP's truthiness, which `if ($x)` and `$x ? :` use: `[]`, `""`, `"0"`, `0` and null are false. */
export function truthy(value) {
  if (Array.isArray(value)) return value.length > 0;
  if (value && typeof value === "object") return Object.keys(value).length > 0;
  return Boolean(value) && value !== "0";
}

/** Whether a value is a PHP list (a JS array) rather than a map. */
export const isList = Array.isArray;

/** PHP's `is_array`: a list or a map. */
export const isArray = (value) => value !== null && typeof value === "object";

/** PHP's `array_values` on a list or a map. */
export const values = (value) => (Array.isArray(value) ? [...value] : isArray(value) ? Object.values(value) : []);

/**
 * PHP's `array_merge($a, $b)` on maps: `$b`'s keys win, in `$a`'s order then
 * `$b`'s new ones, as an object spread.
 */
export const merge = (...maps) => Object.assign({}, ...maps);

/** Option.php's normalizeSeries: a flat list is one series, points one series, else as given. */
export function normalizeSeries(series) {
  const list = values(series);
  if (list.length === 0) return [];
  const first = list[0];
  if (!isArray(first)) return [{ data: list }];
  // PHP's isset: a key that's there but null doesn't count.
  const set = (key) => first[key] !== undefined && first[key] !== null;
  if (!Array.isArray(first) && (set("data") || set("name") || set("value"))) return list;
  return [{ data: list }];
}

/** Option.php's normalizePie: `{ name, value }` as given, else values named by `labels`. */
export function normalizePie(series, labels = []) {
  const list = values(series);
  if (list.length === 0) return [];
  if (isArray(list[0])) return list;
  return list.map((value, i) => ({ value, name: labels[i] ?? String(i) }));
}
