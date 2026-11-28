# RFC 0069 — Filters: a column's own

**Status:** accepted (2026-11-28) · **Target:** 0.63.0 · builds on [RFC 0012](0012-elyra-ui.md), [RFC 0067](0067-totals.md) and [RFC 0068](0068-columns-the-user-picks.md) · see [Decisions](#decisions)

## Summary

The search box finds "Ada" anywhere in a row. It can't say "the open
invoices", "over 10,000", "due this month": the questions a list of
records is asked all day. Today an app that wants them builds its own
controls over `Table`, writes them into `query` by hand, and adds the
`WHERE`s to the resource's index command itself; each app's look and
keys differ, and the generated resources have none.

This RFC gives a column a filter, the table the controls, and
`make:resource` the backend half:

```svelte
<Table
  page={invoices}
  bind:query
  columns={[
    "number",
    { key: "customer", filter: "text" },
    { key: "status", filter: { options: [["open", "Open"], ["paid", "Paid"], ["overdue", "Overdue"]] } },
    { key: "due", filter: "dates" },
    { key: "amount", align: "end", filter: "range", total: "sum" },
  ]}
/>
```

- **`filter`** on a column: `"text"` (contains), `{ options }` (one or
  more of them), `"range"` (a number from, to) or `"dates"` (a day from,
  to). `"options"` alone takes the values the rows have (for `rows`).
- **The control** opens from a funnel on the column's header: a popover
  with the field, Apply and Clear. An active filter shows as a chip over
  the table ("Status: Open, Overdue ×"), and *Clear filters* takes them
  all off.
- **`query.filters`** is where they go: `{ status: ["open", "overdue"],
  amount: { min: 10000 }, due: { from: "2026-11-01", to: "2026-11-30" } }`.
  A `rows` table filters itself; a live `page`'s command gets them, as it
  gets the search and the sort.
- **`make:resource`** writes `filters` into the index command: a
  `FILTERABLE` allowlist per column and kind, and the `WHERE`s, so no
  filter from the frontend reaches SQL unchecked.
- **The totals** (RFC 0067), the export (RFC 0066) and the summary are
  over what's filtered.

## Motivation

- **Lists are filtered more than they're searched.** "Overdue", "this
  month", "over a limit" are a filter each; a search can't say them.
- **The backend has to do it.** A live page of 4,000 invoices can't be
  filtered in the window; the command must, and safely.
- **One way for every list.** Each app building its own filter bar is
  each app's keys, chips and words, and an assistant that can't tell what
  a list is filtered by.
- **The generated resources are the first lists an app has.** If they
  can't filter, every app writes the same code after `make:resource`.

## Current state (0.63, with RFCs 0066–0068)

- `Table`: `search` (a box, `query.search`), `sortable` (`query.sort`,
  `query.direction`), paging (`query.page`); `rows` does them itself, a
  live `page`'s command does them from `query`.
- `make:resource`'s index command: `{Ty}Query { search, sort, direction,
  page, per_page }`, `SORTABLE` and `SEARCHABLE` allowlists, `paginate`.
- The query builder has `where_in`, `where_gte`, `where_lte`,
  `where_between`, `where_like`, `when_some`.

## Design

### A column's `filter`

| `filter` | The control | `query.filters[key]` | Matches |
|---|---|---|---|
| `"text"` | a field | `"ada"` | contains, ignoring case |
| `{ options: [[value, label], …] }` | checkboxes | `["open", "overdue"]` | one of them |
| `"options"` | checkboxes of the values in `rows` | the same | the same |
| `"range"` | two number fields, from and to | `{ min?, max? }` | at least, at most |
| `"dates"` | a [`DateRangePicker`](../ui/date-picker.md) | `{ from?, to? }` (ISO days) | on or after, on or before |

An empty filter is no filter (and isn't in `query.filters`).

### The controls

A sortable header is a button that sorts; a filterable one gets a second,
small button beside its label: a funnel, filled when the column is
filtered, named "Filter Status" (and "Status, filtered" when it is). It
opens a [`Popover`](../ui/popover.md) under the header with the control,
*Apply* (Enter) and *Clear*; Esc closes it, focus back on the funnel.

Over the table, beside the search, a chip a filter, said as what it is:
"Status: Open, Overdue", "Amount: 10,000 or more", "Due: Nov 1 – Nov 30".
A chip's × clears it; *Clear filters* clears every one. The chips are a
list named "Filters", and each change is said ("3 of 412 shown" for
`rows`, the page's total for a live one).

### Where they go

`query.filters` is written with `page: 1`, as a search is. A `rows` table
filters before it searches and sorts; a live `page` re-runs its query
with them. The totals (RFC 0067) and the export (RFC 0066) are of the
filtered rows; the table's summary for the assistant says the filters.

### The backend: `make:resource`

The index command's query gets `filters`:

```rust
pub struct InvoiceQuery {
    pub search: Option<String>,
    pub filters: InvoiceFilters,
    // sort, direction, page, per_page as before
}

/// The list's filters, one a filterable column: anything else is ignored.
pub struct InvoiceFilters {
    pub status: Option<Vec<String>>,
    pub amount: Option<Range<f64>>,
    pub due: Option<Dates>,
    pub customer: Option<String>,
}
```

`Range { min, max }` and `Dates { from, to }` are `elyra::db::filter`'s,
with `apply` helpers (`where_gte`, `where_lte`, `where_in`, `where_like`
with `%` and `_` escaped). The struct is typed from the model's fields,
so a filter on a field the model doesn't have, or of the wrong kind,
doesn't compile and can't be sent: serde drops unknown keys, and the
generated TypeScript types say what's there. `make:resource` makes a
filter for each field by its type (an enum or a string with few values:
options; a number: range; a date: dates; other text: text), and the
generated list's columns have `filter` to match.

### Words

`elyra.table.filter` ("Filter :column"), `filtered` (":column,
filtered"), `apply`, `clearFilter`, `clearFilters`, `filters`
("Filters"), `atLeast`, `atMost`, `between`, `shown` (":count of :total
shown"), in English and Norwegian.

### Testing

- Each kind over `rows`: text, options (given and from the rows), range,
  dates; empty ones dropped; with the search and the sort.
- The popover's keys and focus; the chips and their ×; *Clear filters*;
  the live region.
- A live `page`: `query.filters` written with `page: 1`.
- Totals and the export over the filtered rows.
- The generated command: each kind's `WHERE`, `%` and `_` in a text
  filter, an unknown key ignored, a model test over SQLite (and the
  MySQL/Postgres job).
- A browser check in the docs: filter a status and a range, clear one.

## Implementation plan

1. `internal/filters.js`: matching rows, a filter's text for its chip,
   held by tests without a DOM.
2. `Table`'s funnels, popovers and chips; `query.filters`; the words.
3. `elyra::db::filter` (`Range`, `Dates`, the helpers), with tests.
4. `make:resource`: the filters struct, the `WHERE`s, the columns'
   `filter`; its tests and the `rata new` smoke test.
5. Docs (`docs/ui/table.md`, `docs/resources.md`), CHANGELOG.

## Open questions

1. **Funnels on the headers, or a filter bar over the table?**
   *Recommendation:* funnels, with the chips over the table. The filter
   sits on the column it filters, the chips say what's on, and a table
   with ten filterable columns doesn't grow a bar of ten fields.
2. **`"options"` from the rows' values, for `rows`?**
   *Recommendation:* yes, for `rows` only; a live page has a page of the
   values, not all of them, so it needs `{ options }`.
3. **A typed filters struct, or a map of column to filter?**
   *Recommendation:* typed. It's the allowlist and the types at once, and
   the TypeScript side gets it from codegen; a map needs both written by
   hand.
4. **Kept with `persist`, as the columns are?**
   *Recommendation:* no. A filter is a question asked now; a list that
   opens filtered by last week's question hides rows. (Saved views are a
   later RFC.)
5. **Filters in the Laravel package's resources too?**
   *Recommendation:* not in this RFC. The table sends `query.filters` to
   any backend; the Laravel side is its own change, if apps ask.

## Decisions

1. **Funnels on the headers**, and chips over the table; no filter bar.
2. **`"options"` from the rows' values**, for `rows` only.
3. **A typed filters struct** in the generated command.
4. **Filters aren't kept** with `persist`.
5. **No filters in the Laravel package** in this RFC.

## Not in this RFC

- **Saved views** (a named set of filters, columns and sort).
- **Filters joined by OR**, or on related records.
- **Filters in the Laravel package.**

## Alternatives considered

- **A filter bar of fields over the table**: always there, and as wide
  as the columns that filter.
- **Only the search, with a syntax** (`status:open amount:>10000`): fast
  for some, unknown to most, and nothing for the keyboard to land on.
- **A map of filters in the command**: an allowlist and a type per key by
  hand, in every resource.
