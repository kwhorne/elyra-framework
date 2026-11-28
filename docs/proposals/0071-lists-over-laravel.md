# RFC 0071 — Lists over Laravel: filters, totals and the search, served

**Status:** accepted (2026-11-28) · **Target:** 0.64.0 · builds on [RFC 0005](0005-laravel-backend.md), [RFC 0067](0067-totals.md) and [RFC 0069](0069-filters.md) · see [Decisions](#decisions)

## Summary

0.63's lists filter by their columns and total them in their foot, and a
list over the app's own database gets both from `make:resource`. A list
over a Laravel backend gets neither: RFC 0069 left "filters in the Laravel
package" for later, and RFC 0067 reads a page's `totals` that no Laravel
answer has. Worse, a filter set on such a list doesn't arrive at all:
`Backend::query` encodes a flat struct, and `filters[amount][min]` isn't
flat. An app with a Laravel backend (Félagi is one) builds its lists'
filters by hand on both sides, or goes without.

This RFC carries a list's question to Laravel and its answer back:

```php
// routes/api.php → InvoiceController@index
public function index(Request $request)
{
    return Invoice::query()
        ->elyraList($request, [
            'search' => ['number', 'customer'],
            'sort' => ['number', 'due', 'amount'],
            'filters' => ['status' => 'options', 'amount' => 'range', 'due' => 'dates', 'customer' => 'text'],
            'totals' => ['amount' => 'sum'],
        ]);
}
```

- **The desktop sends the filters**: `Backend::query` encodes nested
  values as PHP reads them (`filters[status][]=open&filters[amount][min]=10000`),
  and `make:resource --backend` writes a typed `{Model}Filters` into the
  index command's query, as RFC 0069 did for a local one.
- **`elyraList`** in the Laravel package: one builder macro that applies
  the search, the filters, the sort and the page from the request, each
  through an allowlist, and paginates; with `totals`, the page's answer
  carries `totals` over every row that matched, which the `Table`'s foot
  shows (RFC 0067) rather than "This page".
- **The same kinds as RFC 0069**: text (contains, `%` and `_` as
  themselves), options, a range, days; a key or a kind the allowlist
  doesn't name is ignored, never reaches SQL.

## Motivation

- **Most lists in a Laravel-backed app are the server's.** The filters and
  totals of 0.63 stop at the window's edge for them.
- **The query string has to be right on both sides.** PHP's brackets are
  what Laravel reads; the desktop has to write them, and the server has to
  take only what it allows.
- **Every controller would write the same thirty lines**: a search over
  some columns, a sort through an allowlist, a cap on `per_page`, the
  filters by kind, and a sum over the same `WHERE`. One macro, tested once.
- **A page's sum isn't the list's.** "This page" is honest, but the sum
  the user wants is of every matching invoice; only the server can give it
  without sending every row.

## Current state (0.63)

- `make:resource --backend` writes `{Ty}Query { search, sort, direction,
  page, per_page }` and sends it with `Backend::get(PATH).query(&query)`;
  reqwest's `query` takes a flat struct (`serde_urlencoded`).
- The Laravel package has no list helper; its skill says a list sends
  `search`, `sort`, `direction`, `page` and `per_page`, and the controller
  `->paginate()`s.
- `Table` reads `page.totals` (RFC 0067) and sends `query.filters`
  (RFC 0069).

## Design

### The desktop: nested query values

`BackendRequest::query` encodes with brackets where a value is nested: a
map is `key[sub]=…`, a list `key[]=…`, `null` and empty lists left out;
flat values as before. So `{ search: "ada", filters: { status: ["open",
"overdue"], amount: { min: 10000 } } }` is
`search=ada&filters[status][]=open&filters[status][]=overdue&filters[amount][min]=10000`.
(The brackets are percent-encoded on the wire; PHP decodes them.)

`make:resource --backend`'s query gets `filters: Option<{Ty}Filters>`,
typed by the fields as RFC 0069's, and the view's columns their
`filter`; its generated test checks the URL a filter makes.

### The server: `elyraList`

A `Builder` macro (`Eloquent\Builder::macro('elyraList', …)`), registered
by the package's provider:

| Option | Takes from the request | Does |
|---|---|---|
| `search` | `search` | `where (a like ? or b like ?)`, the term's `%` and `_` escaped |
| `sort` | `sort`, `direction` | `orderBy`, only a listed column, else the key; then the key |
| `filters` | `filters[...]` | per kind: `text` contains, `options` `whereIn`, `range` `>=`/`<=`, `dates` by day (`to` before the next day) |
| `totals` | — | `sum`/`avg`/`min`/`max`/`count` over the matched rows, before paging |
| `per_page` | `per_page` | capped (`max_per_page`, 100 by default) |

It returns a `LengthAwarePaginator` whose JSON has `totals` beside `data`
and the paginator's own keys, so the `Table`'s foot says "Total" for them.
An API Resource collection takes it too (`Resource::collection(...)` with
`additional(['totals' => ...])`, which the macro's `totals()` gives).

### What isn't let through

A filter key not in `filters`, or a kind not its own (a `range` sent for
an `options` column), is ignored, as an unknown `sort` is. Values are
bound, never interpolated; `text` escapes `%` and `_` (`ESCAPE '!'`, as
RFC 0069's `where_contains`).

### Testing

- Rust: the bracket encoding (maps, lists, nested, `null`, empty, flat
  values unchanged), and `make:resource --backend`'s filters struct and
  its URL test; the smoke test with `--backend` sends a filter.
- Against a real Laravel (`scripts/laravel-backend.sh`, the CI job "Laravel
  backend (real app)", which installs this repository's package): its
  `customers` index uses `elyraList`, and a new `laravel_lists` test, from
  the desktop through `Backend`, checks each kind of filter; an unknown key
  and a wrong kind ignored; the search's `%` and `_`; the sort allowlisted;
  `per_page` capped; and `totals` over every match, not the page. The
  package has no test suite of its own, and this keeps it that way.

## Implementation plan

1. `BackendRequest::query` with nested values, and its tests.
2. `make:resource --backend`: the filters struct, the view's filters, the
   test; the skill's and the docs' lists.
3. The package's `elyraList` macro and `totals`; the real app's
   `customers` index on it, and `laravel_lists`; the skill
   (`elyra-desktop-backend`) shows it.
4. Docs (`docs/backend.md`, `docs/resources.md`), CHANGELOG.

## Open questions

1. **A macro on the builder, or a trait on the model?**
   *Recommendation:* a builder macro. It works on any query (a scope, a
   relation, `Invoice::where(...)`), and a model needs nothing added.
2. **Totals in the paginator's JSON, or in a header?**
   *Recommendation:* in the JSON, beside `data`, where `Table` reads
   them (`page.totals`); a header would need the runtime to carry it.
3. **The allowlist by kind, or by column only (the kind from the
   column's type)?**
   *Recommendation:* by kind, written in the controller. The server's
   column types don't say "options" from "text", and the controller is
   where the API's contract is read.
4. **`make:resource --backend` writes the Laravel controller too?**
   *Recommendation:* no, as today: it writes the desktop's half and says
   what the API serves; the skill shows the controller with `elyraList`.
5. **Saved views (RFC 0070) on the server?**
   *Recommendation:* not in this RFC; they stay on the machine.

## Decisions

1. **A builder macro**, `elyraList`, not a trait on the model.
2. **Totals in the page's JSON**, beside `data`, where `Table` reads them.
3. **The allowlist by kind**, written in the controller.
4. **`make:resource --backend` doesn't write the Laravel controller**; the
   skill shows it with `elyraList`.
5. **No views on the server** in this RFC.

## Not in this RFC

- **Filters on relations** (`whereHas`) and OR between filters.
- **Writing the Laravel controller** from `make:resource`.
- **Views kept on the server.**

## Alternatives considered

- **JSON in one query parameter** (`filters={"status":["open"]}`): easy to
  send, and every controller decodes and validates it by hand; Laravel's
  `$request->input('filters.status')` reads brackets already.
- **`serde_qs` as a dependency**: does the encoding, with more than we
  need; the bracket form is thirty lines and tests.
- **Each controller by hand**: the thirty lines, thirty times, each with
  its own idea of escaping.
