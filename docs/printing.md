# Printing

An Elyra app prints the window, or a part of it, with the system's print
dialog, the same on macOS, Windows and Linux, and makes a PDF of it without
one ([RFC 0062](proposals/0062-printing.md)). What prints is made for paper: the
light theme, no app chrome, `@elyra/ui`'s components in their print form.

```svelte
<script>
  import { print, onPrint } from "@elyra/runtime";
  import { Button } from "@elyra/ui";

  let invoice;
  // File → Print… (⌘P) prints the invoice, not the list around it.
  onPrint(() => print({ element: invoice }));
</script>

<Button onclick={() => print()}>Print…</Button>
<Button onclick={() => print({ element: invoice })}>Print the invoice…</Button>

<article bind:this={invoice}>…</article>
```

## `print(options?)`

- **The system's dialog** for the window that asked: wry's print, an
  `NSPrintOperation` on macOS (where a page's own `window.print()` does
  nothing), WebView2's on Windows, GTK's on Linux. It resolves once the
  dialog is up; whether the user printed isn't said.
- **`window.print()`** is routed here in the shell, so code that already
  calls it prints on macOS too.
- **`element`** prints that part of the page alone: everything around it
  is hidden while it prints, and the part is let out of its scroller and
  its dialog.
- **Outside the shell** (a browser, `rata dev` in one), it's the browser's
  print, with the same preparation.
- Under `Capability::Window` (`/__window/print`); a read-only window, a
  Quick Look, may print.

## `pdf(options?)`

The page, or a part of it, as a PDF without a dialog: what `print()` would
print, laid out by the webview on pages of the size asked for.

```svelte
<Button onclick={async () => files.save(await pdf({ element: invoice }), { defaultName: "Invoice 1042.pdf" })}>
  Save as PDF…
</Button>
```

| Option | Default | |
|---|---|---|
| `element` | the whole page | As `print()`'s. |
| `paper` | `"a4"` | Or `"letter"`. |
| `landscape` | `false` | Turned on its side. |
| `margins` | 10 mm each | `{ top, right, bottom, left }`, in millimetres. |

- **How**: an `NSPrintOperation` that saves to a file (macOS), WebView2's
  `PrintToPdf` (Windows), WebKitGTK's print to GTK's file printer (Linux);
  the shell reads the file back and removes it. The bytes come as they are
  (`application/pdf`).
- **The page is put back** as soon as the PDF is made: there's no dialog to
  wait for.
- **Backgrounds** print where the page asks (`print-color-adjust: exact`),
  as on paper.
- **Only in the shell**: in a browser, `pdf()` rejects (a browser makes a
  PDF only through its print dialog).
- Under `Capability::Window` (`/__window/pdf`), like `print`.

## What prints

While it prints, `<html>` has `data-theme="light"` (and
`data-elyra-printing`), so the app's light tokens are what paper gets;
`<html data-print-theme="current">` keeps the window's theme instead. The
page is put back when the dialog is closed (`afterprint`, or the first
touch of the page after it).

- **`data-print="hide"`** on what never prints (a filter bar, a hint);
  **`data-print="only"`** on what only paper shows (a letterhead, a
  signature line).
- **`@elyra/ui`**: `AppShell` prints its content alone, as long as it is
  (no toolbar, sidebar or status bar). `Table` prints the rows it has loaded
  with the head repeated on each page and no row split, without its search,
  selection, actions, paging or resize grips. `Card` and `Stat` aren't
  split across pages. A `Modal` prints as its page, without the room behind
  it.
- **The runtime's own layers** (toasts, the ⌘K palette, the context menu,
  About, the assistant, the inspector) don't print.
- **Charts** ([`@elyra/charts`](charts.md)) redraw at once in the light
  colours, with no animation; a zoomed chart prints its window.
- **Everywhere**: no shadows, no motion; a link to the web shows its
  address.

The app's own CSS adds to this with `@media print`, as on the web.

For a document of pages, with a header, a footer and "Page 2 of 5" on
each, put it in a [`Report`](ui/report.md) (RFC 0064) and print or make a
PDF of `report.element()`: its paper is the dialog's and the PDF's, and its
margins the only ones.

## File → Print…

`SystemItem::Print` in a menu (`.system(SystemItem::Print)`) is "Print…"
with ⌘P / Ctrl+P, in the framework's words (`elyra.menu.print`). It asks
the window in front: with `onPrint` handlers, they run; without, the window
prints. Only a menu that adds it has it: an app with nothing worth printing
doesn't offer ⌘P.

```rust
Submenu::new("File")
    .item_accel("file.new", "New", "CmdOrCtrl+N")
    .separator()
    .system(SystemItem::Print)
```
