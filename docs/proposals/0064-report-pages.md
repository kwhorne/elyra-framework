# RFC 0064 — Report pages: a header, a footer and "Page 2 of 5"

**Status:** implemented (2026-11-27) · **Target:** 0.62.0 · builds on [RFC 0062](0062-printing.md) and [`@elyra/ui`](0012-elyra-ui.md) · see [Decisions](#decisions)

## Summary

RFC 0062 prints the page and makes a PDF of it, and left out what paper
documents have on every page: the letterhead at the top, "Invoice 1042 ·
Page 2 of 5" at the bottom, the table's head again where it goes on. CSS
has a way (`@page` margin boxes, running elements) that Chromium has only
lately and WebKit doesn't, so an Elyra app can't rely on it on macOS or
Linux; and the dialogs' own headers print the app's address
(`elyra://localhost`) and the date in the platform's way.

This RFC gives `@elyra/ui` a `Report`: content laid out on pages of a
paper's size, each with the app's header and footer, numbered.

```svelte
<script>
  import { Report } from "@elyra/ui";
  import { pdf, print, files } from "@elyra/runtime";
  let report;
</script>

<Report bind:this={report} paper="a4" margins={{ top: 18, bottom: 16 }}>
  {#snippet header({ page })}
    {#if page === 1}<Letterhead />{:else}<p>Invoice 1042</p>{/if}
  {/snippet}
  {#snippet footer({ page, pages })}
    <p>Acme AS · Org. 912 345 678</p><p>Page {page} of {pages}</p>
  {/snippet}

  <InvoiceLines {lines} />
  <Table rows={lines} … />
  <Totals />
</Report>

<Button onclick={() => print({ element: report.element })}>Print…</Button>
<Button onclick={async () => files.save(await pdf({ element: report.element }), { defaultName: "Invoice 1042.pdf" })}>
  Save as PDF…
</Button>
```

- **Pages, laid out by the component**: its children are measured and
  placed on pages of the paper's size (A4 or letter, in millimetres, less
  the margins and the header and footer), each child whole on a page where
  it fits, a page break before one that doesn't.
- **A table goes on**: an `@elyra/ui` `Table` (or a plain `<table>`) taller
  than the room left is split between rows, its head repeated at the top
  of the next page.
- **`header` and `footer`** are snippets given `{ page, pages }`: a
  letterhead on the first page only, a running title after, "Page 2 of 5".
- **Shown as it prints**: on screen, the pages as sheets with a shadow
  (a print preview of the app's own); on paper and in `pdf()`, one sheet a
  page, the dialog's and the webview's own margins set to none, so the
  report's are the ones there are.
- **`print()` and `pdf()` know it**: given a `Report` as `element`, they
  take its paper and margins, so the dialog and the PDF have the same
  pages as the screen.

## Motivation

- **Invoices, delivery notes, statements, reports**: the documents a
  business app hands out are paged, with a header, a footer and page
  numbers. Without them, `pdf()` (RFC 0062) makes a page, not a document.
- **The platforms disagree**: WebView2 prints Chromium's header and footer
  (the title, the address, the date, "1/5") or none; WKWebView and
  WebKitGTK print none the page can shape. An app that wants its own has
  to lay pages out itself, which is what this does once, for all of them.
- **What's printed should be seen first**: a report on screen as its pages
  is the preview the system dialog can't give an app's own layout.

## Current state (0.61)

- `print({ element })` and `pdf({ element, paper, landscape, margins })`
  (RFC 0062) print or make a PDF of a part of the page, the webview
  paginating it.
- `@elyra/ui`'s print forms: `Table`'s head repeated by the browser on
  each page (`table-header-group`), `Card` and `Stat` unsplit.
- No component lays out pages; nothing knows a page's number.

## Design

### `Report`

| Prop | Default | |
|---|---|---|
| `paper` | `"a4"` | Or `"letter"`. |
| `landscape` | `false` | |
| `margins` | 15 mm each | `{ top, right, bottom, left }`, in millimetres: around the header, the body and the footer. |
| `header`, `footer` | — | Snippets, given `{ page, pages }`. |
| `children` | — | The body, in order. |
| `scale` | `"fit"` | On screen: the pages fitted to the width, or `1` for their size. |

`report.element` is what `print` and `pdf` take; `report.pages` is how many
there are (bindable, for "5 pages" beside a button).

### Laying out

After the body renders (and again when it changes, or the fonts load), the
component measures each direct child at the page's width (in CSS pixels,
96 to the inch) and fills pages: a child that fits goes on the page; one
that doesn't starts the next; one taller than a whole page is split if it
can be (a table, between rows; a list, between items) and else placed
alone, clipped to its page, with a warning in the console naming it. The
header and footer of each page are rendered with its number, and measured
first, so a taller first-page letterhead leaves less room on page 1.

A table split across pages repeats its `<thead>`; `@elyra/ui`'s `Table` is
split from the rows it has loaded, without its controls (its print form).

### Paper

On screen the pages are sheets on the inset surface; under `@media print`
each sheet is exactly a page (`break-after: page`, no shadow), and the
runtime's print sheet sets `@page { margin: 0; size: <paper> }` while a
report prints, so the report's own margins are the only ones. `print` and
`pdf` given a report take its paper, orientation and margins (`pdf`'s own
options win where given).

### What it says

The report is a `region` named by its first heading; each page is a
`group` named "Page 2 of 5" (the framework's words, `elyra.report.page`),
so a screen reader can go page by page.

### Testing

- The layout, as a function of measured heights: children on pages, a
  child that doesn't fit moved, a table split between rows with its head
  again, a too-tall child alone; page 1's taller header.
- `{ page, pages }` in the snippets; `pages` bound.
- `print`/`pdf` with a report: its paper and margins in the request.
- A browser check: the gallery's invoice of 120 lines on 4 pages, the
  pages' heights the paper's, and `page.pdf()` with `preferCSSPageSize`
  giving the same 4.

## Implementation plan

1. The layout function (heights in, pages out) and its tests.
2. `Report`: rendering pages, measuring, re-laying out; the snippets.
3. Splitting tables (and lists).
4. The print sheet's `@page`, and `print`/`pdf` taking a report's paper.
5. Docs (a page in the reference), the example app's invoice, CHANGELOG.

## Open questions

1. **The component's own pages, or the platforms' headers and footers?**
   *Recommendation:* its own. The platforms' differ, and two of three
   can't print an app's own header at all.
2. **What splits across pages: tables and lists, or any block?**
   *Recommendation:* tables and lists. A paragraph split mid-line needs
   line measuring the webview doesn't expose; a block taller than a page
   is rare in a report and goes alone, clipped, with a warning.
3. **Children measured as direct children only?**
   *Recommendation:* yes: the body's direct children are the units, so
   an app groups what must stay together in one element.
4. **On screen: pages, or the content as one long column?**
   *Recommendation:* pages, scaled to the width: what's printed is seen.
   `scale={1}` for the real size.
5. **Where: `@elyra/ui`, or a package of its own?**
   *Recommendation:* `@elyra/ui`: it knows `Table`, the tokens and the
   print forms, and it's tree-shaken.

## As built

- **`internal/pages.js`**: `paginate(blocks, room)`, heights in and pages
  out, held by tests without a layout; `unitsOf` finds what splits a block
  (its one table's body rows, or its one list's items).
- **The body renders once, out of sight** (`.source`, at the body's width,
  `aria-hidden`); the pages are copies of its blocks (`cloneNode`, ids
  removed), a split table's copy keeping only its part's rows. It's laid
  out again, debounced, when the body changes (a `MutationObserver`), the
  paper does, or the fonts load; a change during a layout is laid out
  after it.
- **Each page's header and footer are measured** after they render, and the
  pages laid out again until the room stops changing (four passes at
  most), so a taller letterhead on page 1 leaves less room there.
- **Paper is light whatever the window is**: `.elyra-paper` in the tokens
  takes the light theme's values, on the sheets and the source (so a chart
  in a report draws in the light colours). A `Table` inside it is in its
  print form on screen too.
- **`print()` and `pdf()`** read a report's `data-elyra-report="a4
  portrait"`: `print` adds `@page { size; margin: 0 }` while it prints,
  `pdf` asks for its paper with no margins.
- **`report.element()`** is a method, not a property: a Svelte component
  exports functions.
- Checked: in Chrome, a 60-line invoice on three A4 pages (22, 25 and 13
  rows), the same three in its PDF; in an Elyra window on macOS, `pdf()`
  of it in WKWebView, three A4 pages, the header, footer and the table's
  head on each.

## Decisions

1. **The component's own pages**, not the platforms' headers and footers.
2. **Tables and lists split across pages**; a paragraph isn't split.
3. **The body's direct children are the units.**
4. **Pages on screen**, scaled to the width; `scale={1}` for their size.
5. **In `@elyra/ui`.**

## Not in this RFC

- **A report designer** (drag fields onto a page), templates, mail merge.
- **Splitting text** mid-paragraph; footnotes; a table of contents.
- **Headers and footers in a plain `print()`** without a report.
- **Page sizes beyond A4 and letter**, and labels (several to a sheet).

## Alternatives considered

- **CSS paged media** (`@page` margin boxes, `position: running()`): not
  in WebKit, so not on macOS or Linux.
- **The platforms' headers and footers**: WebView2's only, in Chromium's
  form, and with the app's internal address.
- **A PDF library laying out the report** (pdfmake, jsPDF): a second
  layout engine with its own styles; the report wouldn't look like the
  app, and couldn't use its components.
- **paged.js** (a polyfill of paged media): large, and replaces the
  document while it runs; this needs only whole blocks and table rows.
