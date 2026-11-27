# RFC 0062 — Printing: a page, a part of it, a PDF

**Status:** accepted (2026-11-27), `print` implemented; `pdf` next · **Target:** 0.62.0 · builds on [files](0009-files.md), [`@elyra/ui`](0012-elyra-ui.md), [menus that change](0055-menus-that-change.md) and [RFC 0060](0060-exporting-a-chart.md) · see [Decisions](#decisions)

## Summary

Desktop apps print: an invoice, an order's packing list, the report behind a
dashboard, a customer's history for a meeting. In an Elyra app today,
`window.print()` does nothing on macOS (WKWebView has no print for the
page's script), opens WebView2's own dialog on Windows, and nothing in
Elyra's runtime, its components or its menus knows printing exists. What
comes out, where it does, is the dark window as it is: the sidebar, the
toolbar, a table's current page, a chart in the dark theme's colours.

This RFC gives the page printing, and a PDF:

```svelte
<script>
  import { print, pdf, files } from "@elyra/runtime";
</script>

<Button onclick={() => print()}>Print…</Button>
<Button onclick={() => print({ element: invoice })}>Print the invoice…</Button>
<Button onclick={async () => files.save(await pdf({ element: invoice }), { defaultName: "Invoice 1042.pdf" })}>
  Save as PDF…
</Button>

<article bind:this={invoice}>…</article>
```

- **`print(options?)`** opens the system's print dialog for the window
  (wry's print operation: `NSPrintOperation` on macOS, WebView2's on
  Windows, GTK's on Linux), the same on the three. `window.print()` is
  routed to it, so code that already calls it works.
- **`element`** prints a part of the page: everything else is hidden while
  it prints, and comes back.
- **Print looks like paper**: the light theme's tokens, no shadows, no
  app chrome (`AppShell`'s sidebar, toolbar and status bar), `@elyra/ui`'s
  components in their print form (a `Table` with every row and its header on
  each page, a chart redrawn in the light colours, a `Modal`'s page and not
  its backdrop).
- **`pdf(options?)`** gives the page, or a part, as PDF bytes without a
  dialog (WKWebView's `createPDF`, WebView2's `PrintToPdf`, WebKitGTK's
  print operation to a file), for `files.save`, an attachment, or the
  backend.
- **The menu**: `SystemItem::Print` (⌘P / Ctrl+P) in an app's File menu
  asks the page, which prints the window unless it says otherwise
  (`onprint`).

## Motivation

- **Business apps print.** The apps Elyra is for (an ERP's orders, a
  clinic's journal, a shop's stock) hand paper to people who don't use the
  app: a driver, an auditor, a customer at the counter.
- **macOS can't print at all.** `window.print()` is silently ignored by
  WKWebView; the only way is the window's print operation, which only the
  shell can start.
- **What prints should be designed for paper**, and a framework with its own
  components and tokens is where that's done once: every app otherwise
  writes its own `@media print` against markup it doesn't own.
- **A PDF is the other half.** "Save as PDF…" in the print dialog works on
  macOS and (with Microsoft Print to PDF) on Windows, but an app that emails
  an invoice or uploads it to the backend needs the bytes, not a dialog.

## Current state (0.60)

- wry 0.55 has `WebView::print()` on the three platforms
  (`print_with_options` with margins on macOS); Elyra doesn't call it.
- `window.print()`: nothing on macOS; WebView2's print preview on Windows;
  WebKitGTK's dialog on Linux.
- No print stylesheet in `@elyra/ui`; the tokens have no print values; the
  charts read the theme on `data-theme` and the OS's appearance, not on
  printing.
- The shell already does per-platform webview calls of this kind:
  `shell/picture.rs` (RFC 0026) takes a window's picture with
  `takeSnapshot`, `CapturePreview` and `get_snapshot`.
- `files.save` (RFC 0060) writes bytes the page made where the user chose.

## Design

### The runtime

```ts
print(options?: { element?: Element; margins?: Margins }): Promise<void>;
pdf(options?: { element?: Element; paper?: "a4" | "letter"; landscape?: boolean; margins?: Margins }): Promise<Uint8Array>;
type Margins = { top?: number; right?: number; bottom?: number; left?: number }; // millimetres
```

- `print` posts `/__sys/print`; the shell runs wry's print on the window's
  webview, on the main thread (a `WindowAction::Print`). It resolves when
  the dialog is shown (the system's dialog doesn't tell whether the user
  printed).
- In the shell, the runtime sets `window.print = () => print()`.
- Outside the shell (a browser, the tests), `print` is `window.print()` and
  `pdf` rejects with what's missing.
- `pdf` posts `/__sys/pdf` and gets the bytes back (MessagePack `bin`, as
  `file.write` takes them): `createPDFWithConfiguration` on macOS 11+,
  `ICoreWebView2_7::PrintToPdf` on Windows and a WebKitGTK
  `PrintOperation` with `export-filename` on Linux, each into a temporary
  file, read back and removed. Under the `system` feature; a read-only window
  (RFC 0041) may print and make a PDF.

### A part of the page

`element` marks it (`data-elyra-printing`) and its ancestors, and a print
stylesheet the runtime adds hides every other element; the marks go on
`afterprint` (and after the PDF is made). A page can also mark what never
prints: `data-print="hide"`, and `data-print="only"` for what only paper
shows (a letterhead, a signature line).

### What prints in `@elyra/ui`

An `@media print` sheet in the tokens and the components:

- The light tokens, whatever the window shows; no shadows, no animations,
  borders as hairlines; links show their address.
- `AppShell`, `Sidebar`, `StatusBar`, toolbars, `Toast`s, the assistant and
  the inspector are hidden.
- `Table`: every row of what's loaded (its own scroller lets go), the head
  repeated on each page (`display: table-header-group`), rows not split;
  the selection column hidden.
- `Card`, `Stat`: not split across pages.
- `Modal`, `Drawer`: printed as the page they are, without the backdrop,
  when the print is of them (`element`).
- A chart (RFC 0058): on `beforeprint`, redrawn in the light tokens as SVG
  (crisp at any scale), and back on `afterprint`; a zoomed chart (RFC 0061)
  prints its window.

### The menu

`SystemItem::Print` ("Print…", ⌘P / Ctrl+P, in the framework's words) sends
the page a `print` event; `onPrint(handler)` in the runtime takes it (to
print an invoice rather than the list around it); with no handler, the
window prints.

### Testing

- Rust: `/__sys/print` and `/__sys/pdf` behind `system`; a read-only window
  allowed; the bytes as `bin`; on macOS, a PDF of a test page starts with
  `%PDF` (a test that needs the GUI, as `picture.rs`'s do).
- Runtime: `window.print` routed in the shell, untouched outside it;
  `element` marks and unmarks, on `afterprint` and on failure.
- `@elyra/ui`: the print sheet hides the chrome, `Table` lets go of its
  scroller, the light tokens under `print` (happy-dom's `matchMedia`).
- Charts: redrawn on `beforeprint` in the light colours, back after.
- A browser check: `page.emulateMedia({ media: "print" })` on the example
  app, and `page.pdf()` from Chrome as a stand-in for what the webviews
  render.

## Implementation plan

1. `/__sys/print` and `WindowAction::Print`; `print()` and `window.print`
   in the runtime; `SystemItem::Print` and `onPrint`.
2. `element`, `data-print`, and the runtime's print sheet.
3. `@elyra/ui`'s print forms and the light tokens under print; charts on
   `beforeprint`.
4. `/__sys/pdf` on the three platforms; `pdf()`.
5. Docs (a "Printing" page), the example app's invoice, CHANGELOG.

## Open questions

1. **Light for print, always?**
   *Recommendation:* yes, by default; `data-print-theme="current"` on
   `<html>` keeps the window's theme for an app that wants it. Dark paper
   wastes toner and reads badly.
2. **`pdf()` in this RFC, or printing first and the PDF later?**
   *Recommendation:* in this RFC. It's the half apps ask for (emailing an
   invoice), and the shell has done the same per-platform calls before
   (`picture.rs`).
3. **Headers and footers (page numbers, the title, the date)?**
   *Recommendation:* the print dialog's own for `print`; for `pdf`, none in
   this RFC: CSS `@page` margins boxes aren't in the three webviews, and
   drawing them ourselves is a feature of its own.
4. **`SystemItem::Print` in the default menu, or only when the app adds
   it?**
   *Recommendation:* only when added (`.system(SystemItem::Print)`), as the
   other system items: an app with nothing worth printing shouldn't offer
   ⌘P.
5. **`Table`: what's loaded, or every row of the query?**
   *Recommendation:* what's loaded. A live query of 50,000 rows isn't paper;
   an app that means all of them loads them (or makes a PDF from a report
   page of its own).

## Decisions

1. **Light for print** by default; `data-print-theme="current"` on
   `<html>` keeps the window's theme.
2. **`pdf()` is in this RFC**, on the three platforms.
3. **Headers and footers**: the print dialog's own for `print`; none for
   `pdf` in this RFC.
4. **`SystemItem::Print` only when the app adds it**, as the other system
   items.
5. **`Table` prints what's loaded**, not every row of the query.

## Not in this RFC

- **Print preview** inside the app (the system's dialog has one on macOS
  and Windows).
- **Printing without a dialog** (to a chosen printer, silently): a kiosk's
  or a label printer's need, with permissions questions of its own.
- **Reports** (a designer, templates, paged layout beyond CSS).
- **Headers and footers** in `pdf`.

## Alternatives considered

- **Leave it to `window.print()`**: macOS doesn't print, and every app
  writes its own print CSS against `@elyra/ui`'s markup.
- **A PDF library in the page** (jsPDF, pdfmake): a second layout engine,
  and the PDF wouldn't look like the page; the webview already lays it out.
- **The backend makes the PDF** (Laravel and a headless Chrome): right for
  the web, but a desktop app offline has no backend, and the window already
  has the page.
