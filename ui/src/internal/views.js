// Saved views (RFC 0070): a list's question under a name, its filters, its
// search, its sort and its columns. Plain functions, held by tests without
// a DOM.

const same = (a, b) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);
/** Keys in order, so two objects with the same filters compare the same. */
const sorted = (o) => (o && typeof o === "object" && !Array.isArray(o) ? Object.fromEntries(Object.keys(o).sort().map((k) => [k, sorted(o[k])])) : o);
/** A plain copy: Svelte's state proxies don't go through `structuredClone`. */
const clone = (o) => JSON.parse(JSON.stringify(o));
const blank = (s) => (typeof s === "string" ? s.trim() : "") || null;
const filtersOf = (f) => (f && typeof f === "object" && Object.keys(f).length ? sorted(f) : null);

/**
 * The query a view asks: its filters and search, and its sort (else the
 * list's own, `base`), from the first page.
 */
export function queryOf(view, query, base = {}) {
  const next = { ...query, page: 1 };
  next.filters = view?.filters && Object.keys(view.filters).length ? clone(view.filters) : null;
  next.search = view?.search ?? null;
  next.sort = view?.sort ?? base.sort;
  next.direction = view?.sort ? (view.direction ?? "asc") : base.direction;
  if (next.sort === undefined) delete next.sort;
  if (next.direction === undefined) delete next.direction;
  return next;
}

/**
 * Whether the list still asks what its view asks: the same filters, search
 * and sort, and, when the view has columns, the same columns.
 */
export function matches(view, query, choice, base = {}) {
  if (!view) return !filtersOf(query?.filters) && !blank(query?.search);
  const want = queryOf(view, {}, base);
  if (!same(filtersOf(query?.filters), filtersOf(want.filters))) return false;
  if (blank(query?.search) !== blank(want.search)) return false;
  if ((query?.sort ?? null) !== (want.sort ?? null)) return false;
  if ((query?.sort ? (query.direction ?? "asc") : null) !== (want.sort ? (want.direction ?? "asc") : null)) return false;
  if (view.columns && choice) {
    if (!same(view.columns.hidden ?? [], choice.hidden ?? []) || !same(view.columns.order ?? [], choice.order ?? [])) return false;
  }
  return true;
}

/** A view of what the list asks now, under `name`: the columns too, when the user picks them. */
export function viewOf(name, query, choice) {
  const view = { name };
  const filters = filtersOf(query?.filters);
  if (filters) view.filters = clone(filters);
  const search = blank(query?.search);
  if (search) view.search = search;
  if (query?.sort) {
    view.sort = query.sort;
    view.direction = query.direction ?? "asc";
  }
  if (choice) view.columns = { hidden: [...(choice.hidden ?? [])], order: [...(choice.order ?? [])] };
  return view;
}

/** The user's views as the settings store kept them: named ones, each once. */
export function keptViews(saved) {
  if (!Array.isArray(saved)) return [];
  const seen = new Set();
  return saved.filter((v) => {
    if (!v || typeof v !== "object" || typeof v.name !== "string" || !v.name.trim() || seen.has(v.name)) return false;
    seen.add(v.name);
    return true;
  });
}
