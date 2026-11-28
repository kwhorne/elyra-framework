# RFC 0066 — Taking a table out: copy and CSV

**Status:** accepted (2026-11-28) · **Target:** 0.63.0 · builds on [RFC 0012](0012-elyra-ui.md) and [RFC 0060](0060-exporting-a-chart.md) · see [Decisions](#decisions)

## Summary

The first thing a user does with a table of invoices, after reading it, is
take it somewhere else: paste three rows into an email, the month into a
spreadsheet, the list to the accountant as a file. A chart can do that
since 0.60 (RFC 0060); `@elyra/ui`'s `Table` can't. Selecting the text
with the pointer picks up the checkboxes, the action buttons and the
pager, and loses the columns.

This RFC gives `Table` the same way out:

```svelte
<Table rows={invoices} columns={columns} label="Invoices" select bind:this={table} />

<Button onclick={() => table.save()}>Save as CSV…</Button>
```

- **A context menu** on the table (a right click, Shift+F10 or the Menu
  key): *Copy* (the row, or the selected rows), *Copy table* and *Save
  as CSV…*.
- **⌘C / Ctrl+C** on a focused row copies it, or the selected rows: as
  tab-separated text, which pastes into Excel, Numbers and Sheets as cells
  and into an email as lines.
- **`csv()`, `copy(rows?)` and `save()`** on the component, for an app's own
  button.
- **`exportable={false}`** turns it off, as on a chart.

## Motivation

- **A table is the data the user came for.** A desktop app that can't put
  its rows on the clipboard sends the user to a screenshot.
- **The pointer can't do it.** A table with checkboxes, cell snippets and
  actions selects as noise; a paged table selects one page.
- **The format is a spreadsheet's.** What's pasted should land in cells, as
  numbers a spreadsheet can sum, not "kr 12 400,00" as text.
- **Charts and tables sit together.** A dashboard whose chart exports and
  whose table doesn't is half a feature, and the CSV should be the same
  CSV (separator, decimal sign, quoting) from both.

## Current state (0.62)

- `@elyra/charts` makes CSV in `internal/export.js` (`csvOf`,
  `separatorFor`: `;` where the locale's decimal sign is a comma), and saves
  through `files.save` and copies through `clipboard.writeText`
  (RFC 0060).
- `Table` has `rows` (searched, sorted and paged in the table) or `page` (a
  live query's page), `columns` with `format`, `select`/`bind:selected`,
  and keys on the rows (↑/↓, Enter, Space for Quick Look). It has no
  context menu.
- The runtime's clipboard takes text and images, not HTML.

## Design

### What's taken

| | `rows` | `page` (a live query) |
|---|---|---|
| *Copy table*, `csv()`, `save()` | every row, as searched and sorted | the page that's loaded |
| *Copy* | the focused row, or the selected ones | the same |

The columns are the table's, in its order, with their labels as the head;
the `actions` column isn't one. A value is the row's own, not the cell
snippet's drawing: a number as a plain number in the locale's decimal sign
(`12400,5` in Norwegian, no grouping, no currency), a date-only ISO string
as it is, anything else through the column's `format` if it has one, else
as text. So a spreadsheet sums the amounts, and the customer's name is
what the table showed.

### The formats

- **Copy** is tab-separated text, a row a line, the head first when it's
  the table, not when it's rows: a pasted row lands under the user's own
  head. A tab or a line break in a value is a space.
- **CSV** is RFC 0060's: the separator by the locale, fields quoted where
  needed, `\r\n`, and the table's `label` as the first line, as a chart's
  caption is. The file is named by the label and the date ("Invoices
  2026-11-28.csv").

The CSV code moves to `@elyra/ui` (`internal/csv.js`); `@elyra/charts`
uses it from there, so both write the same file.

### The menu and the keys

The menu is the runtime's `contextMenu`, as a chart's. Outside the shell (a
page in a browser) it has *Copy* and *Copy table*, through the browser's
clipboard; *Save as CSV…* needs the shell's dialog. ⌘C is the row's only
when focus is on a row and no text is selected in the page, so copying
text from a cell's input still copies the text. Each copy says "Copied"
in a toast; a failure says why.

### Words

`elyra.table.copy`, `copyTable`, `saveCsv`, `copied` and `exportFailed`,
in English and Norwegian.

### Testing

- The values: numbers plain in the locale, dates, `format`, the actions
  column left out, a `page`'s rows.
- TSV and CSV: separators, quoting, tabs and line breaks in values, the
  head.
- The menu's items in and out of the shell, with and without a selection;
  ⌘C on a row, on the selected rows, and not over selected text.
- `@elyra/charts`' CSV unchanged from the moved code (its tests as they
  are).
- A browser check in the docs: copy rows, paste into a textarea.

## Implementation plan

1. `internal/csv.js` in `@elyra/ui`, from `@elyra/charts`' `export.js`;
   charts imports it.
2. `Table`'s values, `csv()`, `copy()`, `save()`; the menu and ⌘C; the
   words.
3. Docs (`docs/ui/table.md`, an example), CHANGELOG.

## Open questions

1. **A `page` table: the page, or every row of the query?**
   *Recommendation:* the page that's loaded, as printing does (RFC 0062).
   A query of 50,000 rows is an export job of the app's own, through its
   backend; the table can't know what that costs.
2. **Numbers plain, or as the column's `format` shows them?**
   *Recommendation:* plain, in the locale's decimal sign. A spreadsheet
   can't sum "kr 12 400,00"; the format's look is the screen's. A
   column's `format` is used for values that aren't numbers (a status
   code shown as its name), never for a number.
3. **HTML on the clipboard too (a table that pastes with its formatting)?**
   *Recommendation:* not in this RFC. It's a new clipboard call in the shell
   on three platforms, and tab-separated text already lands in cells
   everywhere that matters.
4. **⌘C on by default?**
   *Recommendation:* yes, with `exportable`: it's what a desktop list does,
   and it only acts on a focused row.
5. **Excel (`.xlsx`) as well as CSV?**
   *Recommendation:* no. CSV opens in Excel, Numbers and Sheets; an
   `.xlsx` writer is a dependency for a format the app can make on its
   backend if it needs one.

## Decisions

1. **A `page` table exports the page that's loaded**, not every row of the
   query.
2. **Numbers plain**, in the locale's decimal sign; a column's `format` is
   for values that aren't numbers.
3. **No HTML on the clipboard** in this RFC: tab-separated text.
4. **⌘C on by default**, with `exportable`, on a focused row.
5. **CSV only**, no `.xlsx`.

## Not in this RFC

- **Choosing columns to export**, or exporting hidden ones.
- **Every row of a server-paged query.**
- **Pasting into a table.**

## Alternatives considered

- **A `csv` helper the app calls with its rows**: every app wires its own
  menu and ⌘C, and none of them match.
- **The shown text (the cells' `innerText`)**: what a cell snippet draws
  isn't always text (a badge, an avatar), and numbers come out formatted.
- **Leave it to the browser's selection**: it selects the controls and
  loses the columns.
