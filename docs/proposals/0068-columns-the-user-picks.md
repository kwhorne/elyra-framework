# RFC 0068 — Columns the user picks: shown, hidden, in their order

**Status:** proposed (2026-11-28) · **Target:** 0.63.0 · builds on [RFC 0012](0012-elyra-ui.md), [RFC 0066](0066-taking-a-table-out.md) and [RFC 0067](0067-totals.md)

## Summary

A list of customers has twelve fields worth a column, and each user needs
five of them: sales wants the phone and the last order, accounting the
org number and the balance. Today the app picks the columns of `Table`,
once, for everyone; a user who wants another column asks for a release,
and a narrow window scrolls sideways through columns nobody reads. Every
desktop list (Finder, Outlook, a spreadsheet's hide) lets the user choose.

This RFC lets the user pick, and keeps what they picked:

```svelte
<Table
  rows={customers}
  persist="customers"
  columns={[
    { key: "name", required: true },
    "email",
    "phone",
    { key: "org_number", hidden: true },
    { key: "balance", align: "end", hidden: true, total: "sum" },
  ]}
  choosable
/>
```

- **`choosable`** gives the head a *Columns* button (and the header's
  context menu an item): a menu of checkboxes, one a column, to show or
  hide it, and a way back to the app's columns.
- **The order**: in the same menu, each column moves up or down (by the
  keys too), or a header is dragged to its place.
- **`hidden: true`** starts a column hidden; **`required: true`** keeps one
  shown (the name a row is known by).
- **`persist`** keeps the choice with the widths, in the settings store,
  so the table opens as the user left it.
- **What's hidden is hidden everywhere**: the search still finds it, but
  the export, the totals and print show the columns as the user sees them.

## Motivation

- **The user knows which columns matter to them**, the app's author
  doesn't, and different users of one app want different ones.
- **Width is scarce on a desktop window** that shares the screen; hiding
  three columns is the difference between a list that fits and one that
  scrolls.
- **The choice must last.** A column hidden every morning again is a
  setting the app forgot.
- **The export follows the screen** (RFC 0066): "copy what I see" is what
  a user means; the app's own `csv()` with every column is one line away.

## Current state (0.63, with RFCs 0066 and 0067)

- `columns` are the app's, in its order; `resizable` with `persist` keeps
  the widths under `elyra-ui.table.<persist>` in the settings store.
- The header's cells sort (a button) and resize (a grip); there's no menu
  on the head.
- The table's context menu (RFC 0066) is on its rows: copy and save.

## Design

### The menu

With `choosable`, the head's last cell (the actions' column, or one of its
own) holds a *Columns* button, a `Dropdown` of:

- a checkbox a column, by its label, in the table's order: checked when
  shown; a `required` column's is checked and disabled;
- for the focused item, <kbd>Alt+↑</kbd> / <kbd>Alt+↓</kbd> move the
  column (said: "Phone moved to 3 of 5"), as do up and down buttons on it
  for the pointer;
- *Reset columns*: the app's own columns, order and widths.

A right click on the head opens the same menu. Hiding the last column that
isn't required is refused (the item is disabled): a table shows at least
one.

### Dragging a header

A header's label is dragged left or right to its place (the pointer only;
the keys have the menu). The grip still resizes; a drag that starts on it
isn't a move.

### What's kept

`persist` keeps `{ widths, hidden, order }` under the same key, as one
value: the widths as today (a key's old value, widths alone, is still read
as them), the hidden keys, and the order as keys. A column the app adds
later is shown (unless it's `hidden`) at its place in the app's order; a
key the app took away is forgotten.

### Everywhere else

- **Search** goes on searching every column, hidden ones too: the user
  hid it from view, not from the search. (A hidden match is the row; the
  cell isn't shown.)
- **Sort** by a column that's hidden goes on; its header isn't there to
  say so, so the table's `aria-description` does ("Sorted by balance").
- **Export** (RFC 0066) and **totals** (RFC 0067) are the shown columns,
  in their order; `csv({ all: true })` is every column, in the app's
  order, for an app that wants it.
- **Print and `Report`**: the shown columns. In a `Report` the table is
  as the app set it (a report is a document, not a view).
- **The assistant** sees the shown columns, as the user does.

### Words

`elyra.table.columns` ("Columns"), `resetColumns`, `moveUp`, `moveDown`,
`moved` (":column moved to :at of :count"), `sortedBy`, in English and
Norwegian.

### Testing

- Hide, show, a required column, the last one refused; order by the menu
  and its keys, and by a drag.
- `persist`: kept, read back, the old widths-only value, a column added
  and one taken away.
- Search over a hidden column; sort by one; export, totals and print as
  shown; `csv({ all: true })`.
- A `Report`'s table as the app set it.
- The menu's roles and the live region's words.
- A browser check in the docs: hide two columns, move one, reload.

## Implementation plan

1. `internal/columns.js`: the shown columns from the app's and the user's
   choice, and the kept value, held by tests without a DOM.
2. `Table`'s *Columns* menu, the head's context menu, the drag; `persist`.
3. Export, totals, print and the assistant on the shown columns;
   `csv({ all: true })`.
4. Docs (`docs/ui/table.md`, an example), CHANGELOG; `make:resource
   --view` lists `choosable`.

## Open questions

1. **On with `choosable`, or always?**
   *Recommendation:* with `choosable`. A table of three columns in a
   dialog has nothing to choose, and the button is in its head.
2. **Export: the shown columns, or every one?**
   *Recommendation:* the shown ones, as the user sees them;
   `csv({ all: true })` for the app's own export.
3. **Kept under the widths' key, or one of its own?**
   *Recommendation:* the same key, one value: a table's look is one thing
   to keep, and one key per table is what the app names. The widths-only
   value of 0.62 is read as it was.
4. **Dragging headers in this RFC?**
   *Recommendation:* yes, as well as the menu: it's how every desktop list
   reorders, and the menu covers the keyboard.
5. **`make:resource --view` lists `choosable`?**
   *Recommendation:* yes: a resource's list is the kind of wide list this
   is for, and it has `persist` already.

## Not in this RFC

- **Columns the app doesn't list** (every field of the row, picked by the
  user).
- **Saved views** (named sets of columns, filters and sort).
- **Pinning a column** to the left while the rest scrolls.

## Alternatives considered

- **The app's own settings page** for columns: every app builds it, and
  none of them is where the table is.
- **Only the menu, no drag**: works, but the pointer user reaches for the
  header first.
- **A key of its own for the choice**: a second name per table to keep
  apart, for no gain.
