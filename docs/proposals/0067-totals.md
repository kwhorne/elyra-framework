# RFC 0067 — Totals: a table's foot

**Status:** implemented (2026-11-28) · **Target:** 0.63.0 · builds on [RFC 0012](0012-elyra-ui.md), [RFC 0064](0064-report-pages.md) and [RFC 0066](0066-taking-a-table-out.md) · see [Decisions](#decisions)

## Summary

An invoice ends in a sum; a list of orders, in how many and how much; a
timesheet, in hours. Today `@elyra/ui`'s `Table` has no foot: the app sums
its rows itself and draws the result under the table, where it doesn't
line up with the column it sums, a screen reader doesn't hear it as part
of the table, a `Report` can put it on a page of its own, and a copied or
saved table leaves it out. The invoice in `docs/ui/report.md` does exactly
that (`<p><strong>Total: …</strong></p>`).

This RFC gives a column a `total`:

```svelte
<Table
  rows={lines}
  columns={[
    "item",
    { key: "qty", align: "end", total: "sum" },
    { key: "price", align: "end", format: kr },
    { key: "amount", align: "end", format: kr, total: "sum" },
  ]}
/>
```

- **`total`** is `"sum"`, `"avg"`, `"min"`, `"max"` or `"count"`, or a
  function of the rows (`(rows) => …`) for anything else (a weighted
  average, a sum of `qty * price`).
- **The foot** is a `<tfoot>` row under the columns, each total in its
  column, formatted by the column's `format` and aligned as it is; the
  first column says "Total" when it has none of its own.
- **Over what**: every row of `rows`, as searched (not only the page); a
  live `page`'s totals from the server when it sends them, else the page's
  own, said as "This page".
- **With a selection**, a second line totals the selected rows:
  "Selected (3)".
- **Out of the app**: printed with the table (once, at the end, in a
  `Report` too), in *Copy table* and the CSV (RFC 0066) as their last
  line.

## Motivation

- **A total belongs to its column.** Drawn under the table, it lines up by
  luck, and wraps differently when the window narrows.
- **A screen reader reads a table's foot as the table's.** A paragraph
  after it is another thing, found or not.
- **Paper needs it in the right place.** In a `Report`, a total after a
  split table can land alone on the next page; as the table's foot it
  stays with its last rows.
- **The selection's sum is the question selection asks.** "How much are
  these three invoices?" is why a user ticks them.

## Current state (0.62, and 0.63's RFC 0066)

- `Table` has `columns` with `format` and `align`, `rows` (searched,
  sorted, paged here) or a live `page` (`{ data, total, per_page, … }`),
  `select`/`bind:selected`, and no `<tfoot>`.
- The print form repeats the head on each page (`thead` as a header
  group); a `Report` splits a table between body rows and copies the head
  to each part (`internal/pages.js`).
- RFC 0066's CSV and *Copy table* are the head and the rows.

## Design

### `total`

| `total` | Is |
|---|---|
| `"sum"` | the sum of the column's numbers |
| `"avg"` | their mean |
| `"min"`, `"max"` | the least, the greatest |
| `"count"` | how many rows have a value in the column |
| `(rows, column) => value` | the app's own, given the rows the total is over |

A value that isn't a number (a gap, `null`, text) is left out of sum, avg,
min and max. A sum is rounded to the most decimals of its values, so
`0.1 + 0.2` is `0.3`, not `0.30000000000000004`. A total is formatted by
the column's `format` (`count` isn't: it's a number of rows, formatted as
a number).

### The foot

A `<tfoot>` with a row a total line: its first cell a row header
(`<th scope="row">`) saying what it is ("Total", "This page",
"Selected (3)"), unless the first column has a total of its own, when the
words go in the selection column (with `select`) or before the total
("Total: 12"). The foot's cells are bold, on the surface colour, with a
line above; they don't sort, select or open.

### Over what

- **`rows`**: the rows as searched, every page: the foot doesn't change
  with the page, and it changes with the search ("Total" of what's
  found).
- **A live `page`**: `page.totals` if the server sends it (`{ amount:
  128400, qty: 340 }`, by column key), said as "Total"; else the page's
  rows, said as "This page", so a sum of 25 of 4,000 rows isn't taken for
  all of them. A function `total` is given the page's rows.
- **A selection** (`select`): a second line, "Selected (3)", over the
  selected rows the table has (every page of `rows`; the loaded page of a
  live one); not shown with nothing selected.

### Paper and export

- **Print**: the foot is printed once, after the last row (`tfoot` as a
  row group, not repeated on every page as browsers do by default). The
  selection's line isn't printed.
- **`Report`**: a split table's foot goes with its last part; the parts
  before it have none.
- **RFC 0066**: *Copy table*, `copy()`, `csv()` and `save()` end with the
  foot's first line, its numbers plain as the rows' are; *Copy row* and
  `copy(rows)` don't have it.

### Words

`elyra.table.totalRow` ("Total"), `pageTotal` ("This page") and
`selectedTotal` ("Selected (:count)"), in English and Norwegian.

### Testing

- Each kind of total, a function, gaps and text left out, the rounding,
  `format` (and `count` without it).
- Over a search, over pages; a live page with and without `totals`; the
  selection's line.
- The foot's markup: `tfoot`, the row header, a first column with a total.
- Print CSS (once, not repeated); `Report`'s split, foot on the last part.
- The CSV and *Copy table* with the foot; *Copy row* without.
- The invoice example in `docs/ui/report.md` with its total in the foot;
  a browser check of it as a PDF's last page.

## Implementation plan

1. `internal/totals.js`: the totals of rows by columns, held by tests
   without a DOM.
2. `Table`'s `<tfoot>`, its lines, its words; print CSS; RFC 0066's export.
3. `Report`'s split keeping the foot on the last part.
4. Docs (`docs/ui/table.md`, the invoice), CHANGELOG.

## Open questions

1. **A live page without `totals`: the page's sum, or none?**
   *Recommendation:* the page's, said as "This page". A list that shows a
   page is read a page at a time, and saying so keeps it honest; an app
   whose server sends `totals` gets the whole query's.
2. **The selection's line: always with `select`, or opt in?**
   *Recommendation:* always, when something is selected and a column has a
   total. It's what the user ticked the rows to find out.
3. **The foot in the CSV and *Copy table*?**
   *Recommendation:* yes, as the last line. A sum pasted without its total
   is a sum the user retypes; a spreadsheet that recomputes it ignores a
   line it doesn't need.
4. **`page.totals` from the Laravel package's resources too?**
   *Recommendation:* not in this RFC. The table reads it when it's there;
   a resource that sends it is a backend change of its own, if apps ask.
5. **Group subtotals (a total per customer)?**
   *Recommendation:* not in this RFC; it needs grouped rows first.

## As built

- **`internal/totals.js`**: `totalOf`, `totalsOf` and `totalText`, plain
  functions; an average is rounded to its values' decimals and two more.
- **A column's `format`** is given the total and a row that has only it
  (`{ amount: 27600 }`), so a format that reads its row doesn't fail.
- **The first column's own total** is in its row header after the words
  ("Total: 15"), not in the selection column, which is too narrow for
  them and isn't printed.
- **The foot** is there when a column has a total and the table has rows
  (not while it loads, or when nothing matches).
- **`Report`**: a split table's copies before the last lose their
  `<tfoot>`; the room a part takes is measured with it, so a page is at
  most a foot's height short.
- **Print**: `tfoot` is a row group (`display: table-row-group`), printed
  once after the last row; the selection's line is hidden on paper and in
  a report.
- **Checked in Chromium**: the `Report` docs' invoice, three sheets, the
  foot on the third only, under its columns ("Total 150 … 27 600 kr").

## Decisions

1. **A live page without `totals`** has the page's own, said as "This
   page".
2. **The selection's line** shows whenever something is selected and a
   column has a total.
3. **The foot is in the CSV and *Copy table***, as their last line.
4. **No `page.totals` from the Laravel package** in this RFC.
5. **No group subtotals** in this RFC.

## Not in this RFC

- **Grouped rows and subtotals.**
- **Totals the user picks** (a menu on the foot to switch sum to avg).
- **A foot without totals** (free content under the columns): a `Report`
  or the app's own markup does that.

## Alternatives considered

- **A `foot` snippet** the app fills: the app still sums, aligns and
  formats, and the export, print and `Report` don't know what's in it.
- **Totals as a last row in `rows`**: sorted, searched, selected and paged
  with the data, and exported as one.
- **The server's totals only**: a plain `rows` table has no server, and
  most invoices are one.
