# Report

A document of pages ([RFC 0064](../proposals/0064-report-pages.md)): its
content laid out on sheets of A4 or letter, each with the app's header and
footer, numbered. An invoice, a delivery note, a statement: what's printed
and saved as a PDF, and seen as its pages first.

<!-- example: report/invoice -->
```svelte
<script>
  import { Report, Table } from "@elyra/ui";

  const lines = Array.from({ length: 60 }, (_, i) => {
    const qty = (i % 4) + 1;
    const price = 40 + ((i * 37) % 300);
    return { item: `Part ${i + 1}`, qty, price, amount: qty * price };
  });
  const kr = (n) => `${n.toLocaleString("nb")} kr`;
</script>

<Report paper="a4" margins={{ top: 14, bottom: 12 }}>
  {#snippet header({ page })}
    {#if page === 1}<h1>Invoice 1042</h1><p>Acme AS · Storgata 1 · 0155 Oslo</p>{:else}<p>Invoice 1042</p>{/if}
  {/snippet}
  {#snippet footer({ page, pages })}
    <p class="foot">Acme AS · Org. 912 345 678 <span>Page {page} of {pages}</span></p>
  {/snippet}

  <p>Ada Lovelace · Analytical Engines Ltd</p>
  <!-- The total is the table's foot: it goes with the last rows, on the last page. -->
  <Table
    rows={lines}
    columns={[
      "item",
      { key: "qty", align: "end", total: "sum" },
      { key: "price", align: "end", format: kr },
      { key: "amount", align: "end", format: kr, total: "sum" },
    ]}
    label="Lines"
    perPage={100}
  />
</Report>

<style>
  .foot { display: flex; justify-content: space-between; color: var(--elyra-muted); font-size: 12px; }
</style>
```

- **The body's direct children are the units**: each goes whole on the
  page where it fits, else on the next. Wrap what must stay together in
  one element.
- **A table or a list goes on**: taller than the room left, it's split
  between its rows (its head again on the next page, its foot, a `Table`'s
  [totals](table.md#totals), only after the last) or its items. A
  `Table` in a report is in its print form, on screen too: no search,
  selection, actions or paging.
- **`header` and `footer`** are given `{ page, pages }`: a letterhead on
  the first page, a running title after, "Page 2 of 5". Each page's are
  measured, so a taller letterhead leaves less room on page 1.
- **A block taller than a page** goes alone, clipped, with a warning in the
  console.
- **On screen** the pages are sheets, fitted to the width (`scale={1}` for
  their size), always light. They're copies of the body, laid out again
  when it changes: a report is for reading and printing, not for typing in.

## Printing it

`print()` and `pdf()` ([printing](../printing.md)) take a report's element
and its paper: the dialog and the PDF have the same pages as the screen,
with the report's margins and no others.

```js
import { pdf, print, files } from "@elyra/runtime";

print({ element: report.element() });
files.save(await pdf({ element: report.element() }), { defaultName: "Invoice 1042.pdf" });
```

## Accessibility

Each sheet is a group named "Page 2 of 5" (`elyra.report.page`), so a
screen reader can go page by page. The body it's copied from is hidden
from it.

## API

### Report

| Prop | Type | Default | Description |
|---|---|---|---|
| `paper` | `"a4" \| "letter"` | `"a4"` | The sheet's size. |
| `landscape` | `boolean` | `false` | Turned on its side. |
| `margins` | `{ top?, right?, bottom?, left? }` | `{}` | In millimetres, 15 each where not given: around the header, the body and the footer. |
| `scale` | `"fit" \| number` | `"fit"` | On screen: fitted to the width, or this size. |
| `pages` (bindable) | `number` | `1` | How many pages it is. |
| `header` | `Snippet<[{ page, pages }]>` | — | At the top of each page. |
| `footer` | `Snippet<[{ page, pages }]>` | — | At the bottom of each page. |
| `labels` | `object` | `{}` | Its words, for this instance. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The body. |

`report.element()` is the report's element, for `print` and `pdf`.
