# RFC 0060 — Exporting a chart: an image, a picture to paste, the data

**Status:** draft (2026-11-27) · **Target:** 0.60.0 · builds on [RFC 0058](0058-elyra-charts.md), [RFC 0059](0059-all-of-wirecharts.md) and [files](0009-files.md) · see [Open questions](#open-questions)

## Summary

A chart in a desktop app is something people take with them: into a
report, an email or a slide. Today they screenshot it. RFC 0058 and 0059
both left export out as "worth doing, separately".

This RFC lets every chart in `@elyra/charts` save itself or copy itself, from
a menu the user already knows, with no code in the app:

```text
right-click a chart (or Shift+F10 on it):
  Copy as image
  Save as PNG…
  Save as SVG…
  ─────────────
  Copy data
  Save data as CSV…
```

```svelte
<!-- On by default; off where a chart shouldn't leave the app -->
<SankeyChart {nodes} {links} exportable={false} />

<!-- Or from the app's own button -->
<script>
  let chart;
</script>
<LineChart bind:this={chart} {series} {categories} />
<Button onclick={() => chart.save("png")}>Export</Button>
```

- **Images**:
  - **SVG** is the chart as it's drawn.
  - **PNG** is that same SVG at twice the pixels.
  - Both have the theme's surface behind them, so a dark chart stays
    readable on a white page.
- **Copy as image** puts a PNG on the system clipboard, ready to paste into
  Word, Keynote or Mail.
- **The data** is the hidden table the screen reader and the assistant
  already read, as CSV in the app's locale. Norwegian gets decimal commas,
  with semicolons between the fields.
- **The file** is written where the user chose in the save dialog, and
  nowhere else. The page only gets to write to a file the user picked.

## Motivation

- **A report is where a chart ends up.** Today the user screenshots it, and
  gets a blurry crop with the window's edges.
- **The table is already there.** "Copy data" reuses what the screen reader
  and the assistant read (RFC 0058), with no export code per chart.
- **Every app would build the same menu.** It should be built once, and in
  the app's language.

## Current state (0.59)

- **`dialog.save`** gives the page a `ChosenFile` grant of kind `save`. A Rust
  command can write to it, but the page itself can't. Every export would
  need a command of its own.
- **`clipboard.writeText` exists, but there's nothing for images.** `arboard`,
  which the shell already uses for text, can also hold images.
- **`contextMenu(event, items)`** in the runtime draws the themed menu.
- **ECharts can give an SVG string** (`renderToSVGString`, with the SVG
  renderer) or a data URL.

## Design

### In the runtime

```ts
// Save bytes where the user chooses: the dialog, then the write. The path, or null.
files.save(data: Blob | Uint8Array | string, options: SaveDialogOptions): Promise<string | null>;

// An image on the clipboard: PNG bytes.
clipboard.writeImage(png: Blob | Uint8Array): Promise<void>;
```

`files.save` is `dialog.save` followed by a new shell route,
`/__system/file.write { id, bytes }`. That route writes only to a grant of
kind `save` from this run, once. Any other id is refused, as a path the
user didn't choose can't be passed today. It needs `Capability::System`,
like the dialogs, and the `system` feature.

`clipboard.writeImage` decodes the PNG in the shell and hands RGBA to
`arboard`.

### In `@elyra/charts`

- **The menu:**
  - `ChartBase` opens it on `contextmenu`, and on Shift+F10 or the Menu key
    when the chart has focus.
  - Its words are `elyra.charts.*`.
  - Its items follow what the shell can do. Without the `system` feature,
    only "Copy data" is offered (the page's own clipboard), so the menu
    never offers something that fails.
- **The images:**
  - **SVG:** an SVG-rendered chart gives its own SVG. A canvas or 3D chart
    can't give an SVG, so it doesn't offer one.
  - **PNG:** the SVG is drawn onto a canvas at 2× scale. A canvas-rendered
    chart gives its own PNG.
  - **Behind the image:** the theme's `--elyra-surface`, and for an SVG the
    font's family too.
- **The data:**
  - The table, its caption as the first line, then the head row and the
    rows.
  - In the app's locale:
    - numbers as the table writes them, which for Norwegian means decimal
      commas;
    - semicolons between the fields where the locale's decimal sign is a
      comma, otherwise commas;
    - quoted per RFC 4180.
- **The file name** is the chart's `label`, else its summary's first words,
  else "chart", plus the date, for example `Revenue 2026-11-27.png`.
- **For the app:**
  - `exportable={false}` turns the menu off.
  - `bind:this` gives `save(format)`, `copy()` and `csv()` for the app's own
    buttons.

### Testing

- `files.save` and `/__system/file.write`:
  - a save grant is written, once;
  - an open grant, an unknown id or a second write are refused.
- `clipboard.writeImage`: PNG in, RGBA out, in the shell's own test of the
  decode.
- `@elyra/charts`:
  - the menu's items with and without the system feature;
  - the CSV in English and Norwegian;
  - the file name;
  - SVG with the surface behind it;
  - PNG from the SVG, with a fake canvas.
- A browser check: Save as PNG on a chart in the gallery writes a PNG of
  the size asked.

## Implementation plan

1. `/__system/file.write` and `files.save`; `clipboard.writeImage`.
2. `@elyra/charts`: the export functions, the menu, `exportable`, the
   component methods, the words.
3. The gallery's charts with the menu; docs (`charts.md`, `system.md`),
   CHANGELOG.

## Open questions

1. **A context menu, or a visible button on every chart?**
   *Recommendation:* the context menu, with Shift+F10 and the Menu key.
   It's where a desktop user looks, and it doesn't crowd a dashboard of
   small charts. An app that wants a button calls `chart.save()`.
2. **On by default?**
   *Recommendation:* yes. A chart the user can see they can screenshot
   anyway; `exportable={false}` is for the rare one that shouldn't leave.
3. **`files.save` as a general runtime API**, not just the charts'?
   *Recommendation:* yes. "Save this as a file" is what every export
   needs (a report, an invoice PDF made in the page), and it's safe the
   same way `dialog.save` is.
4. **CSV in the app's locale, or always comma and point?**
   *Recommendation:* the app's locale. A Norwegian user opens it in
   Excel, which reads semicolons and decimal commas in a Norwegian setup.
5. **Images on the clipboard on every platform?**
   *Recommendation:* yes, through `arboard` (its default `image-data`),
   on macOS, Windows and X11; on Wayland it needs arboard's
   `wayland-data-control` feature, which the shell turns on for Linux.
   Where it still fails, the item says so in a toast rather than
   disappearing.

## Not in this RFC

- **PDF**: an image in a PDF is the app's report's business; `files.save`
  takes the bytes.
- **Exporting several charts at once** (a dashboard).
- **Drag a chart out** onto the desktop or into another app.

## Alternatives considered

- **ECharts' toolbox `saveAsImage`**: it downloads through the browser,
  which a webview turns into nothing or a file in Downloads without asking,
  and it's English, unthemed and one more thing in every chart's corner.
- **A command per export in Rust**: every app writes the same command, and
  the page still has to hand it the bytes.
