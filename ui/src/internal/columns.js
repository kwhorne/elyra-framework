// Columns the user picks (RFC 0068): which of the app's columns are shown,
// in what order, from the app's own (`hidden`, `required`) and the user's
// choice (`{ hidden, order }`, kept with the widths). Plain functions, held
// by tests without a DOM.

/**
 * The columns as the user has them: `all` in their order (the user's, with
 * a column the app added since at its place in the app's), and `shown`,
 * the ones not hidden. A `required` column is never hidden, and at least
 * one column is always shown.
 */
export function arrange(columns, choice) {
  const byKey = new Map(columns.map((c) => [c.key, c]));
  const order = Array.isArray(choice?.order) ? choice.order.filter((k) => byKey.has(k)) : [];
  const known = new Set(order);
  // A column the user hasn't seen goes after the app's column before it.
  const all = order.map((k) => byKey.get(k));
  columns.forEach((col, i) => {
    if (known.has(col.key)) return;
    const before = columns.slice(0, i).reverse().find((c) => all.includes(c));
    all.splice(before ? all.indexOf(before) + 1 : 0, 0, col);
  });
  const userHidden = new Set(Array.isArray(choice?.hidden) ? choice.hidden : []);
  // The user's choice for the columns they've seen; the app's for the rest.
  const isHidden = (col) => !col.required && (choice && known.has(col.key) ? userHidden.has(col.key) : !!col.hidden);
  let shown = all.filter((col) => !isHidden(col));
  if (shown.length === 0 && all.length) shown = [all[0]];
  return { all, shown };
}

/** The choice after the user shows or hides `key`, from the arrangement they see. */
export function toggled(arranged, key) {
  const shown = new Set(arranged.shown.map((c) => c.key));
  shown.has(key) ? shown.delete(key) : shown.add(key);
  return choiceOf(arranged.all, shown);
}

/** The choice after `key` moves `by` places (−1 up, 1 down), or to `to`. */
export function moved(arranged, key, { by, to } = {}) {
  const keys = arranged.all.map((c) => c.key);
  const from = keys.indexOf(key);
  if (from < 0) return choiceOf(arranged.all, new Set(arranged.shown.map((c) => c.key)));
  const target = Math.max(0, Math.min(keys.length - 1, to ?? from + (by ?? 0)));
  keys.splice(from, 1);
  keys.splice(target, 0, key);
  const byKey = new Map(arranged.all.map((c) => [c.key, c]));
  return choiceOf(
    keys.map((k) => byKey.get(k)),
    new Set(arranged.shown.map((c) => c.key)),
  );
}

/** `{ hidden, order }` for columns in an order, some of them shown. */
function choiceOf(all, shown) {
  return { hidden: all.filter((c) => !shown.has(c.key)).map((c) => c.key), order: all.map((c) => c.key) };
}

/** Whether the user may hide `key`: not a required column, nor the last one shown. */
export function canHide(arranged, key) {
  const col = arranged.all.find((c) => c.key === key);
  if (!col || col.required) return false;
  const shown = arranged.shown.some((c) => c.key === key);
  return !shown || arranged.shown.filter((c) => c.key !== key).length > 0;
}

/**
 * What the settings store kept for a table: `{ widths, hidden, order }`, or
 * 0.62's widths alone (a key per column), read as the widths.
 */
export function keptOf(saved) {
  if (!saved || typeof saved !== "object" || Array.isArray(saved)) return { widths: {}, choice: null };
  const isNew = "widths" in saved || "hidden" in saved || "order" in saved;
  const raw = isNew ? (saved.widths ?? {}) : saved;
  const widths = {};
  if (raw && typeof raw === "object") {
    for (const [k, w] of Object.entries(raw)) if (typeof w === "number" && Number.isFinite(w)) widths[k] = w;
  }
  const choice =
    isNew && (Array.isArray(saved.hidden) || Array.isArray(saved.order))
      ? { hidden: Array.isArray(saved.hidden) ? saved.hidden.map(String) : [], order: Array.isArray(saved.order) ? saved.order.map(String) : [] }
      : null;
  return { widths, choice };
}
