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
| `format` | `(value, row) => string`, for what's shown. |

`sortable` on the table is `true` for every column, or a list of keys.

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
| <kbd>↓</kbd> <kbd>↑</kbd> | On a row that opens: the next row, the previous row. |
| <kbd>Enter</kbd>, <kbd>Space</kbd> | On a row: opens it. On a header: sorts by it. |

## Accessibility

- A real `<table>`, named by `label`, with `scope="col"` headers.
- A sorted header has `aria-sort` (`ascending` or `descending`), and a
  sortable one that isn't sorted has `none`. Each sortable header is a
  button.
- While it loads, the table has `aria-busy`. The placeholder rows are
  hidden from screen readers.
- The actions column's header is named "Actions" for screen readers.
- The pager is a `<nav>` named by `label`. Each page is named by
  `pageLabel(n)`, and the current one has `aria-current="page"`.

## API

### Table

| Prop | Type | Default | Description |
|---|---|---|---|
| `page` | `Readable<{ value, error, loading }> \| Page` | — | A `live.*` store, or a page. |
| `rows` | `object[]` | — | A plain array, searched, sorted and paged in the table. |
| `query` (bindable) | `{ search?, sort?, direction?, page? }` | `{}` | What the user asked for. |
| `columns` | `Array<string \| { key, label?, sortable?, align?, format? }>` | `[]` | |
| `sortable` | `boolean \| string[]` | `false` | Every column, or these keys. |
| `search` | `boolean` | `false` | A search box. |
| `perPage` | `number` | `25` | Rows per page, for `rows`. |
| `key` | `string` | `"id"` | The rows' id field. |
| `label` | `string` | — | The table's name. |
| `onrowclick` | `(row) => void` | — | Makes the rows open: click, or <kbd>Enter</kbd>. |
| `cell` | `Snippet<[row, column]>` | — | Draws a cell. |
| `actions` | `Snippet<[row]>` | — | A last column, per row. |
| `empty` | `Snippet` | — | Instead of the empty list's message. |
| `labels` | `object` | `{}` | Its words, translated. |
| `class` | `string` | `""` | |

### Pagination

| Prop | Type | Default | Description |
|---|---|---|---|
| `page` (bindable) | `number` | `1` | The current page, from 1. |
| `lastPage` | `number` | `1` | |
| `label` | `string` | `"Pagination"` | The pager's name. |
| `previousLabel` | `string` | `"Previous"` | |
| `nextLabel` | `string` | `"Next"` | |
| `pageLabel` | `(n: number) => string` | `` (n) => `Page ${n}` `` | Each page's name. |
| `onchange` | `(page: number) => void` | — | Told the new page. |
| `class` | `string` | `""` | |
