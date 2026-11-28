# Table and Pagination

A list of records: searched, sorted and paged, with rows that open. Give it
a `live.*` page, and the command does the searching and sorting and keeps
the list current. Give it a plain array, and the table does it itself.

<!-- example: table/rows -->
```svelte
<script>
  import { Badge, Table } from "@elyra/ui";

  const CUSTOMERS = [
    { id: 1, name: "Ada Lovelace", email: "ada@analytical.co", credit: 12400, status: "Active" },
    { id: 2, name: "Grace Hopper", email: "grace@cobol.dev", credit: 860, status: "Overdue" },
    { id: 3, name: "Linus Torvalds", email: "linus@kernel.org", credit: 3150, status: "Active" },
    { id: 4, name: "Margaret Hamilton", email: "margaret@apollo.space", credit: 22000, status: "Paused" },
    { id: 5, name: "Edsger Dijkstra", email: "edsger@goto.nl", credit: 0, status: "Active" },
    { id: 6, name: "Barbara Liskov", email: "barbara@clu.mit.edu", credit: 4700, status: "Active" },
  ];
  const TONE = { Active: "success", Overdue: "danger", Paused: "warning" };
  const euros = (n) => new Intl.NumberFormat("en", { style: "currency", currency: "EUR", maximumFractionDigits: 0 }).format(n);

  let query = $state({});
  let opened = $state(null);
</script>

<Table
  rows={CUSTOMERS}
  bind:query
  perPage={4}
  search
  sortable={["name", "credit"]}
  label="Customers"
  columns={["name", "email", { key: "credit", align: "end", format: euros }, "status"]}
  onrowclick={(row) => (opened = row.name)}
>
  {#snippet cell(row, column)}
    {#if column.key === "status"}
      <Badge variant={TONE[row.status]}>{row.status}</Badge>
    {:else}
      {column.format ? column.format(row[column.key], row) : row[column.key]}
    {/if}
  {/snippet}
</Table>
<p>Opened: {opened ?? "none"} · query: <code>{JSON.stringify(query)}</code></p>
```

Search, sort and the page go into `bind:query`, shown under the table.
Clicking a sortable header sorts by it, and a second click reverses the
order. The rows open with a click, or with <kbd>Enter</kbd> on the keyboard.

## Over a live query

This is what `make:resource` lists are. The index command answers a
`Page<T>`, the table writes the user's search, sort and page into `query`,
and the live query runs again with it:

```svelte
<script>
  import { navigate } from "@elyra/runtime";
  import { live } from "./bindings";
  import { Table } from "@elyra/ui";

  let query = $state({ page: 1 });
  const customers = $derived(live.customers_index(query));
</script>

<Table
  page={customers}
  bind:query
  columns={["name", "email", "country"]}
  sortable={["name", "country"]}
  search
  label="Customers"
  onrowclick={(row) => navigate(`/customers/${row.id}`)}
/>
```

`query` has `search`, `sort`, `direction` (`asc` or `desc`) and `page`. The
search is sent after the user stops typing for a moment. The command sorts
by the columns it allows, so list those in `sortable`.

`page` can be a `live.*` store, which gives `{ value, error, loading }`, or a
plain `Page`. While the first page loads, the table shows placeholder rows:

<!-- example: table/live -->
```svelte
<script>
  import { readable } from "svelte/store";
  import { Table } from "@elyra/ui";

  // What `live.customers_index(query)` gives the table: loading, then a page.
  const customers = readable({ value: undefined, error: null, loading: true }, (set) => {
    const timer = setTimeout(() => {
      const data = [
        { id: 1, name: "Ada Lovelace", email: "ada@analytical.co" },
        { id: 2, name: "Grace Hopper", email: "grace@cobol.dev" },
      ];
      set({ value: { data, total: 2, per_page: 25, current_page: 1, last_page: 1 }, error: null, loading: false });
    }, 2000);
    return () => clearTimeout(timer);
  });
</script>

<Table page={customers} columns={["name", "email"]} label="Customers" />
```

When the query fails, the table says so in a `Callout`, over the last rows it
had.

## Columns

`columns` are keys, or objects for more:

| Key | |
|---|---|
| `key` | The field in the row. |
| `label` | The header. Without one, the key reads as one: `created_at` is "Created at". |
| `sortable` | This column, whatever `sortable` on the table says. |
| `align` | `"end"` for numbers and money. |
| `width` | Its width in pixels, to start from. |
| `format` | `(value, row) => string`, for what's shown. |
| `total` | `"sum"`, `"avg"`, `"min"`, `"max"`, `"count"`, or `(rows, column) => value`: in the table's foot ([totals](#totals)). |

`sortable` on the table is `true` for every column, or a list of keys.

## Column widths

With `resizable`, each column's header has a grip at its right edge: drag
it, or focus it and press <kbd>←</kbd> <kbd>→</kbd>. The last column (or
the actions, when there are some) takes the rest, so it has no grip. A
double click or <kbd>Enter</kbd> on a grip takes the column back to its
`width`, or to none.

<!-- example: table/widths -->
```svelte
<script>
  import { Table } from "@elyra/ui";

  const INVOICES = [
    { id: 1, number: "F-1042", customer: "Ada Lovelace AS", note: "Analytical engine, second instalment", total: "12 400 kr" },
    { id: 2, number: "F-1043", customer: "Grace Hopper Consulting", note: "COBOL training, two days in Arlington", total: "8 600 kr" },
    { id: 3, number: "F-1044", customer: "Linus Torvalds", note: "Kernel review", total: "3 150 kr" },
  ];
</script>

<!-- Drag a header's right edge, or focus it and press ← →; the note takes the rest. -->
<Table
  rows={INVOICES}
  label="Invoices"
  resizable
  persist="docs-invoices"
  columns={[{ key: "number", width: 90 }, { key: "customer", width: 200 }, { key: "total", align: "end", width: 110 }, "note"]}
/>
```

- **The first change** keeps every other column at the width it has, so
  only the one dragged moves. From then, the table lays out at the widths
  (`table-layout: fixed`), at least as wide as its box, and scrolls
  sideways when they add up to more. A cell's text that doesn't fit ends
  in "…".
- **`persist="invoices"`** keeps the widths in the app's
  [settings store](../store.md) (under `elyra-ui.table.invoices`) when the
  user lets go, or stops pressing the keys, and the table starts from them
  next time. Give each table its own key. Without the shell (in a
  browser, in the tests) there's no store, and the widths start from the
  columns' own.
- **`make:resource --view`** makes its lists `resizable`, kept under the
  resource's name.

## Cells, actions and an empty list

`cell(row, column)` draws a cell your way. `column` is the column object,
with `key`, `label` and `format`. `actions(row)` adds a last column, and a
click there doesn't open the row. `empty()` replaces the empty list's
message.

<!-- example: table/actions -->
```svelte
<script>
  import { Button, Table } from "@elyra/ui";

  let customers = $state([
    { id: 1, name: "Ada Lovelace", email: "ada@analytical.co" },
    { id: 2, name: "Grace Hopper", email: "grace@cobol.dev" },
  ]);
</script>

<Table rows={customers} columns={["name", "email"]} label="Customers">
  {#snippet actions(row)}
    <Button size="sm" href="/customers/{row.id}">Open</Button>
    <Button
      size="sm"
      variant="danger"
      confirm="Delete {row.name}?"
      onclick={() => (customers = customers.filter((c) => c.id !== row.id))}
    >
      Delete
    </Button>
  {/snippet}
  {#snippet empty()}
    <p>Everyone is gone. Reload the page to bring them back.</p>
  {/snippet}
</Table>
```

## Selecting rows

`select` gives the rows checkboxes, and `bind:selected` is the keys of the
ones selected. A bar above the table says how many, with a way to clear
them and the `bulk` snippet's actions: `bulk(selected, clear)`.

<!-- example: table/select -->
```svelte
<script>
  import { Button, Table } from "@elyra/ui";

  const INVOICES = [
    { id: 2041, number: "2041", customer: "Ada Lovelace", due: "2026-11-05", amount: 12400 },
    { id: 2042, number: "2042", customer: "Grace Hopper", due: "2026-11-07", amount: 860 },
    { id: 2043, number: "2043", customer: "Kristen Nygaard", due: "2026-11-12", amount: 3150 },
    { id: 2044, number: "2044", customer: "Ole-Johan Dahl", due: "2026-11-14", amount: 22000 },
    { id: 2045, number: "2045", customer: "Margaret Hamilton", due: "2026-11-20", amount: 4700 },
  ];
  const euros = (n) => new Intl.NumberFormat("en", { style: "currency", currency: "EUR", maximumFractionDigits: 0 }).format(n);

  let selected = $state([]);
  let sent = $state([]);
</script>

<!-- `select="invoices"`: the assistant's note says which are selected too. -->
<Table
  rows={INVOICES}
  bind:selected
  select="invoices"
  perPage={4}
  label="Invoices"
  columns={["number", "customer", "due", { key: "amount", align: "end", format: euros }]}
>
  {#snippet bulk(ids, clear)}
    <Button size="sm" variant="primary" onclick={() => ((sent = ids), clear())}>Send reminders</Button>
  {/snippet}
</Table>
<p>Selected: <code>{JSON.stringify(selected)}</code> · reminded: <code>{JSON.stringify(sent)}</code></p>
```

- **Across pages and searches:** the selection is keys, not rows, so it
  stays while the user pages or searches. The header's checkbox selects or
  clears the page, and shows when only part of it is selected.
- **Rows that go:** a row a live page no longer has, on the same page and
  query, is no longer selected (deleted in another window, say). From a
  plain array, a row that's left the array is dropped.
- **Clicks:** <kbd>Shift</kbd>+click selects every row from the last one
  clicked. When the rows don't open (no `onrowclick`), a click on a row
  selects it. When they do, the click opens it and the checkbox selects it.
  A click on an input, a button or a link in a cell is theirs.
- **The assistant:** `select="invoices"` (a name, rather than `true`) also
  tells the in-app assistant what's selected, as
  [`assistant.selected("invoices", ids)`](../assistant.md#whats-selected),
  so "postpone these to Friday" needs no ids. It says nothing until
  something is selected, and clears it when the table goes. Only the keys
  go, never the rows.

## Rows not yet sent

A write made with `.optimistic()` ([RFC 0039](../proposals/0039-optimistic-writes.md))
shows in the list at once, before the server has it. `pending` marks those
rows with a small dot at the start, "Not sent yet" to a screen reader and as
a tooltip, until the server confirms it, or the change is taken back:

```svelte
<script>
  import { pending } from "@elyra/runtime";
</script>

<Table page={customers} bind:query columns={["name", "email"]} pending={$pending.customers} />
```

`pending` is a list of the rows' keys (`key`, `id` by default); a number
and the same number as a string match.

## Quick Look

`preview` gives each row a route to show in Quick Look
([RFC 0041](../proposals/0041-quick-look.md)): <kbd>Space</kbd> on a row
shows it in a window beside this one, the list keeps the keyboard, and
<kbd>↓</kbd> <kbd>↑</kbd> move it to the next row and the one before.
<kbd>Space</kbd> closes it, as does <kbd>Esc</kbd> wherever the focus is
in the window (the search box, a row's checkbox), but in a dialog or menu
that <kbd>Esc</kbd> is for; and <kbd>Enter</kbd> opens
the row, as `onrowclick` says. With `select`, <kbd>Space</kbd> selects, so
<kbd>⌘Y</kbd> (<kbd>Ctrl+Y</kbd>) shows it.

```svelte
<Table
  page={customers}
  bind:query
  columns={["name", "email"]}
  onrowclick={(c) => navigate(`/customers/${c.id}`)}
  preview={(c) => `/customers/${c.id}`}
/>
```

The window is read-only: only the app's live commands run in it, and
`can` says no to what only writers need, so the detail view's Edit and
Delete buttons hide. See [windows](../windows.md#quick-look).
`make:resource --view` writes `preview` on its lists.

## Totals

A column's `total` is in the table's foot, under it
([RFC 0067](../proposals/0067-totals.md)):

```svelte
<Table
  rows={lines}
  columns={[
    "item",
    { key: "qty", align: "end", total: "sum" },
    { key: "amount", align: "end", format: kr, total: "sum" },
    { key: "note", total: "count" },
  ]}
/>
```

- **`"sum"`, `"avg"`, `"min"`, `"max"`** are over the column's numbers; a
  gap, `null` or text is left out. A sum has its values' decimals, so
  `0.1 + 0.2` is `0.3`. **`"count"`** is how many rows have a value.
  **A function** is given the rows and the column, for anything else (a
  weighted mean, `qty * price` without a column of its own).
- **Shown** by the column's `format` (given the total, and a row that has
  only it), aligned as the column is; a count is a number of rows, in the
  locale's digits. The first column says "Total", or, when it has a total
  of its own, "Total: 15".
- **Over what**: from `rows`, every row as searched, not only the page;
  from a live `page`, the server's when the page has `totals` (`{ amount:
  128400 }`, by key), else the page's own rows, said as "This page".
- **Selected rows** get a line of their own, "Selected (3)", while any are.
- **On paper** the foot is printed once, after the last row (not at the
  foot of every page), without the selection's line; in a
  [`Report`](report.md), a split table's foot goes with its last part.
- **Taken out**: *Copy table*, `copy()`, `csv()` and `save()` end with the
  foot's first line, its numbers plain; a copied row doesn't have it.

## Taking it out

A table's rows go to a spreadsheet, an email or a file
([RFC 0066](../proposals/0066-taking-a-table-out.md)):

- **The menu**: a right click on the table (or <kbd>Shift+F10</kbd>, the
  Menu key, on a row) has *Copy row* (or *Copy 3 rows*, the selected ones,
  when the row is one of them), *Copy table* and, in the shell, *Save as
  CSV…*. A right click in a field of a cell's own keeps the browser's menu.
- **<kbd>⌘C</kbd> / <kbd>Ctrl+C</kbd>** on a focused row copies it, or the
  selected rows; not while text is selected in the page, which copies the
  text.
- **Copied** is tab-separated text: it pastes into Excel, Numbers and Sheets
  as cells, and into an email as lines. Rows go without the head (so they
  land under the user's own), the table with it.
- **Saved** is CSV in the app's locale (`;` where the decimal sign is a
  comma), the `label` as its first line, named by the label and the date:
  `Invoices 2026-11-28.csv`. The same CSV a chart saves.

<!-- example: table/out -->
```svelte
<script>
  import { Button, Table } from "@elyra/ui";

  const INVOICES = [
    { id: 2041, number: "2041", customer: "Ada Lovelace", due: "2026-11-05", amount: 12400.5 },
    { id: 2042, number: "2042", customer: "Grace Hopper", due: "2026-11-07", amount: 860 },
    { id: 2043, number: "2043", customer: "Kristen Nygaard", due: "2026-11-12", amount: 3150 },
  ];
  const euros = (n) => new Intl.NumberFormat("en", { style: "currency", currency: "EUR" }).format(n);

  let table = $state();
  let selected = $state([]);
</script>

<!-- Right-click a row, or ⌘C on one: the amounts go as 12400.5, not €12,400.50. -->
<Table
  bind:this={table}
  rows={INVOICES}
  bind:selected
  select
  label="Invoices"
  columns={["number", "customer", "due", { key: "amount", align: "end", format: euros }]}
/>
<Button onclick={() => table.save()}>Save as CSV…</Button>
```

What's taken:

- **The rows**: from `rows`, every one, as searched and sorted, not only
  the page; from a live `page`, the page that's loaded (a query of 50,000
  rows is an export of the app's own, through its backend). The selected
  rows from `rows` are taken from every page.
- **The columns**: the table's, in its order, with their labels; not the
  selection's or the actions'.
- **A value** is the row's own, not what a `cell` snippet draws: a number
  plain, in the locale's decimal sign and without grouping (`12400,5` in
  Norwegian), so a spreadsheet sums it; a date (`2026-11-05`) as it is;
  anything else through the column's `format`, or as text.

For an app's own buttons, the table has `csv()` (the CSV as text),
`copy(rows?)` (the rows given, or the table) and `save()` (the path
written, or `null` if the user cancelled), through `bind:this`.
`exportable={false}` takes the menu and <kbd>⌘C</kbd> away.

## Its words

`labels` translates what the table says:

```svelte
<Table
  rows={customers}
  columns={["name"]}
  labels={{
    search: "Søk…",
    failed: "Klarte ikke å hente listen",
    empty: "Ingenting her ennå",
    noMatch: (term) => `Ingen treff for «${term}»`,
    total: (n) => `${n} i alt`,
    previous: "Forrige",
    next: "Neste",
    page: (n) => `Side ${n}`,
    selected: (n) => `${n} valgt`,
    clear: "Fjern valg",
    selectAll: "Velg alle på siden",
    selectRow: (row) => `Velg ${row.name}`,
    resize: (label) => `Endre bredden på ${label}`,
    pending: "Ikke sendt ennå",
    copy: (n) => (n === 1 ? "Kopier raden" : `Kopier ${n} rader`),
    copyTable: "Kopier tabellen",
    saveCsv: "Lagre som CSV…",
    copied: "Kopiert",
    exportFailed: (message) => `Kunne ikke kopiere eller lagre tabellen: ${message}`,
    totalRow: "Sum",
    pageTotal: "Denne siden",
    selectedTotal: (n) => `Valgt (${n})`,
  }}
/>
```

`make:resource --view` writes them in the app's language.

## Pagination

The pager on its own: Previous, Next, the first and the last page, and the
pages around the current one. It shows nothing when there's only one page.

<!-- example: table/pagination -->
```svelte
<script>
  import { Pagination } from "@elyra/ui";

  let page = $state(4);
</script>

<Pagination bind:page lastPage={12} label="Invoices" />
<span>Page {page} of 12</span>
```

## Keyboard

| Key | |
|---|---|
| <kbd>Tab</kbd> | Through the search box, the sortable headers, the rows that open, the actions and the pager. |
| <kbd>↓</kbd> <kbd>↑</kbd> | On a row that opens, or with `select`: the next row, the previous row. |
| <kbd>Enter</kbd>, <kbd>Space</kbd> | On a row: opens it. On a header: sorts by it. With `preview`, <kbd>Space</kbd> shows the row in Quick Look instead, and closes it. |
| <kbd>⌘Y</kbd> / <kbd>Ctrl+Y</kbd> | With `preview` and `select`: Quick Look. |
| <kbd>Space</kbd> | With `select`: selects the row, or unselects it. <kbd>Shift</kbd>+<kbd>Space</kbd> selects from the last one. |
| <kbd>⌘A</kbd> / <kbd>Ctrl+A</kbd> | With `select`, on a row: selects the page. |
| <kbd>⌘C</kbd> / <kbd>Ctrl+C</kbd> | On a row: copies it, or the selected rows ([taking it out](#taking-it-out)). |
| <kbd>Shift+F10</kbd>, Menu | On a row: the menu to copy or save. |
| <kbd>Esc</kbd> | With Quick Look open: closes it, wherever the focus is in the window. With `select`, on a row: clears the selection. |
| <kbd>←</kbd> <kbd>→</kbd> | With `resizable`, on a grip: the column 16 px narrower or wider (64 with <kbd>Shift</kbd>). |
| <kbd>Home</kbd> | On a grip: the column as narrow as it goes (48 px). |
| <kbd>Enter</kbd> | On a grip: the column back to its own width. |

## On paper

Printed ([printing](../printing.md)), a table is the rows it has loaded, at
the page's width: its head again at the top of each page, no row split
across two, and none of what's for the hand: the search, the selection
column and the bulk bar, the actions, the paging and the resize grips. A
row the user selected isn't tinted. To print every row of a long query,
load them first (or print a report page of the app's own).

## Accessibility

- A real `<table>`, named by `label`, with `scope="col"` headers.
- A sorted header has `aria-sort` (`ascending` or `descending`), and a
  sortable one that isn't sorted has `none`. Each sortable header is a
  button.
- While it loads, the table has `aria-busy`. The placeholder rows are
  hidden from screen readers.
- The actions column's header is named "Actions" for screen readers.
- With `select`, each row's checkbox is named by `selectRow(row)` ("Select
  Ada", from the first column), and the header's by `selectAll`. The bar
  is a region named by how many are selected, and the count is
  `aria-live`.
- With `resizable`, each grip is a focusable `separator`, vertical, named
  by `resize(label)` ("Resize Customer"), with the width as its value.
- The pager is a `<nav>` named by `label`. Each page is named by
  `pageLabel(n)`, and the current one has `aria-current="page"`.

## API

### Table

| Prop | Type | Default | Description |
|---|---|---|---|
| `page` | `Readable<{ value, error, loading }> \| Page` | — | A `live.*` store, or a page. |
| `rows` | `object[]` | — | A plain array, searched, sorted and paged in the table. |
| `query` (bindable) | `{ search?, sort?, direction?, page? }` | `{}` | What the user asked for. |
| `columns` | `Array<string \| { key, label?, sortable?, align?, width?, format?, total? }>` | `[]` | |
| `sortable` | `boolean \| string[]` | `false` | Every column, or these keys. |
| `search` | `boolean` | `false` | A search box. |
| `perPage` | `number` | `25` | Rows per page, for `rows`. |
| `key` | `string` | `"id"` | The rows' id field. |
| `label` | `string` | — | The table's name. |
| `onrowclick` | `(row) => void` | — | Makes the rows open: click, or <kbd>Enter</kbd>. |
| `cell` | `Snippet<[row, column]>` | — | Draws a cell. |
| `actions` | `Snippet<[row]>` | — | A last column, per row. |
| `empty` | `Snippet` | — | Instead of the empty list's message. |
| `select` | `boolean \| string` | `false` | Checkboxes on the rows. A name also tells the assistant what's selected. |
| `selected` (bindable) | `Array<string \| number>` | `[]` | The selected rows' keys. |
| `bulk` | `Snippet<[selected, clear]>` | — | Actions for the selected rows, in the bar. |
| `resizable` | `boolean` | `false` | Grips to set the columns' widths. |
| `persist` | `string` | — | With `resizable`, a key to keep the widths under, in the settings store. |
| `pending` | `Array<string \| number>` | — | The keys of rows with a change not yet sent: a dot on each. |
| `preview` | `(row) => string` | — | A row's route for Quick Look: <kbd>Space</kbd> shows it beside the window (<kbd>⌘Y</kbd> with `select`). |
| `exportable` | `boolean` | `true` | The menu and <kbd>⌘C</kbd> that copy or save its rows ([taking it out](#taking-it-out)). |
| `labels` | `object` | `{}` | Its words, translated. |
| `class` | `string` | `""` | |

### Pagination

| Prop | Type | Default | Description |
|---|---|---|---|
| `page` (bindable) | `number` | `1` | The current page, from 1. |
| `lastPage` | `number` | `1` | |
| `label` | `string` | — | The pager's name. In the app's language by default (`elyra.pagination.*`). |
| `previousLabel` | `string` | — | In the app's language by default (`elyra.pagination.*`). |
| `nextLabel` | `string` | — | In the app's language by default (`elyra.pagination.*`). |
| `pageLabel` | `(n: number) => string` | — | Each page's name. In the app's language by default (`elyra.pagination.*`). |
| `onchange` | `(page: number) => void` | — | Told the new page. |
| `labels` | `object` | `{}` | Its words, for this instance: the app's translations otherwise ([its words](../i18n.md#the-frameworks-own-words)). |
| `class` | `string` | `""` | |
