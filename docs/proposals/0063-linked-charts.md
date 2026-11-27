# RFC 0063 — Linked charts: one window, one pointer, a selection

**Status:** implemented (2026-11-27) · **Target:** 0.62.0 · builds on [RFC 0058](0058-elyra-charts.md) and [RFC 0061](0061-zoom.md) · see [Decisions](#decisions)

## Summary

A dashboard is several charts over the same time: revenue, orders, returns,
one above the other. Zoom one to March (RFC 0061) and the others still
show the year; point at the 14th in one and the others say nothing; and
there's no way to pick out "these points" in a scatter and have the table
beside it show only them. Today an app wires that itself, through
`bind:range` and `onselect`, chart by chart.

This RFC links charts, and lets the user select:

```svelte
<LineChart series={revenue} categories={days} zoom link="sales" />
<ColumnChart series={orders} categories={days} zoom link="sales" />

<ScatterChart series={customers} brush bind:selection />
<Table rows={selection?.points ?? rows} … />
```

- **`link="name"`**: charts with the same name share their window (zoom
  one, the others follow) and their pointer (hover the 14th in one, each
  shows its tooltip at the 14th). Matched by the axis's values, not by
  position, so charts with different days still line up where they meet.
- **`brush`**: drag a rectangle over a scatter, or a span along a category
  or time axis, to select what's under it. **`bind:selection`** is what's
  selected, as data the app uses: the points, or the categories.
- **The keyboard selects too**: on the chart, Space marks where a selection
  starts, ←/→ (or the arrow keys over a scatter's points) extend it, Space
  again ends it, Esc clears it.
- **What it says**: the summary and the hidden table mark the selection, as
  they mark the window (RFC 0061); linked charts say they're linked.

## Motivation

- **Dashboards are linked by nature.** "Revenue and orders in March" is one
  question asked of two charts; zooming each by hand is two answers that
  drift.
- **Selection is the scatter's question.** "Who are these customers, up
  here with high revenue and few orders?" A scatter that can't select
  points can only be looked at.
- **The app owns the data, so the selection must be data.** ECharts' brush
  reports indexes per series; an app wants the points (or the categories)
  to filter a table, open records or export.
- **The keyboard can't drag.** Selecting by the pointer alone leaves a
  keyboard user out of the scatter's main question.

## Current state (0.61)

- `bind:range` (RFC 0061) gives each chart its window; linking two is the
  app's `$effect`, and the pointer isn't shared at all.
- `onselect` reports one point on a click or Enter.
- ECharts has `connect(group)` (shared tooltips and data zoom, by index),
  and the `brush` component (rectangle, polygon, line along an axis)
  reporting `{ seriesIndex, dataIndex[] }` on `brushSelected`.

## Design

### `link`

ChartBase keeps a registry of linked charts by name (module scope, the
page's). When one moves its window, the others are set to the same
`range` in their own axis's values (RFC 0061's `windowOf`), so a chart of
weeks and one of days stay together; a value one chart lacks is snapped to
the nearest it has. When the pointer is over a category in one, the others
`showTip` at the same value; leaving one hides the others'. Not ECharts'
`connect`, which matches by index.

`link` and `bind:range` together: the range is the group's.

### `brush`

| Chart | Brush | `selection` |
|---|---|---|
| `ScatterChart` | a rectangle (and lasso with Shift) | `{ points: [{ series, x, y, size? }] }` |
| Line, area, column, bar | a span along the category axis | `{ from, to, categories: [...] }` |

The brush is ECharts' (`BrushComponent`, its toolbox hidden), in the
tokens' colours; `brushSelected` is turned into `selection` from the
chart's own data, so the app gets its points, not indexes. Setting
`selection` (`null` to clear) draws it. A click on empty space clears it.
With `brush` and `zoom` both, a drag brushes and the wheel zooms.

### The keyboard

On a chart with `brush`, focused: Space starts a selection at the focused
point or category, ←/→ extend it along the categories (over a scatter,
the arrows move to the nearest point in their direction, and Space toggles
each one into the selection), Space ends it, Esc clears it. Each change is
said in a live region: "March 3 to March 14 selected, 12 days".

### What it says

The summary adds the selection ("Selected: 12 of 340 customers"); the
table marks the selected rows (`aria-selected`), as the window's are
marked (`aria-current`). A linked chart's description says with what:
"Linked with 2 other charts".

### Testing

- `link`: zoom one, the others' `dataZoom` by value; different categories
  snapped; the pointer's tooltip on each; unlinking on unmount.
- `brush`: a `brushSelected` turned into points and into categories;
  `selection` in draws the brush; clear.
- The keys: a span over categories, points toggled over a scatter, Esc.
- The summary and the table's marks; the live region's words.
- A browser check in the gallery: two linked charts and a brushed scatter.

## Implementation plan

1. `link`: the registry, the window shared by value, the pointer.
2. `brush` on the scatter and along the category axis; `bind:selection`.
3. The keys and the live region; the summary and the table.
4. The gallery's linked dashboard; docs, CHANGELOG.

## Open questions

1. **`link` by a name, or by passing a shared object
   (`link={group}`)?**
   *Recommendation:* a name. It reads in markup, needs no import, and two
   charts on different pages of the same window can share it.
2. **The pointer shared by value, or by index (ECharts' `connect`)?**
   *Recommendation:* by value: linked charts are often over different
   spans (a month's days beside a year's), where index 14 means different
   days.
3. **Lasso on the scatter, or only the rectangle?**
   *Recommendation:* the rectangle, and the lasso with Shift held: the
   rectangle is what most selections are, and the lasso costs nothing more
   from ECharts.
4. **`selection` as data (points, categories) or as indexes?**
   *Recommendation:* data, with `index` on each point as well, for an app
   that keeps its rows in the same order.
5. **Selection across linked charts?**
   *Recommendation:* not in this RFC. Linked charts share the window and
   the pointer; a selection is one chart's, and the app passes it on.

## As built

- **`internal/link.js`** keeps the groups (module scope, the page's); each
  member follows a window and a pointer, and is told when the group
  changes, for "Linked with 2 other charts" (`aria-description`). The
  window goes by `range`, each chart snapping it to its own axis
  (`snapRange`, `nearestIndex`); the echo of a window a chart was given
  isn't passed on, and the pointer's echo is quiet for 120 ms.
- **The pointer** is ECharts' `updateAxisPointer` turned into the axis's
  value, and `showTip` at the nearest point on the others; `globalout`
  hides theirs. A chart without a window reads its axis once a drawing,
  not on each move.
- **`internal/brush.js`**: ECharts' brush (`BrushComponent`, no toolbox,
  armed by `takeGlobalCursor`), `brushSelected` turned into the app's
  points or categories, a selection the app sets drawn as areas. Its fill
  is the accent at 15 % (`--elyra-accent-soft` is solid, and would hide the
  points). With `zoom`, the inside zoom no longer pans on a drag.
- **The keys over a scatter** walk the points with ←/→ (series by series,
  as the tooltip does) rather than to the nearest in a direction; Space
  marks each with ECharts' `select`.
- **The table's selected rows** say ", selected" in their header:
  `aria-selected` isn't allowed on a plain table's rows.
- Checked in the gallery's Linked section: a wheel over one chart zooms
  both, the pointer shows in both, a dragged rectangle selects 33 of 80.

## Decisions

1. **`link` by a name.**
2. **The pointer shared by value**, not by index.
3. **The rectangle on the scatter, the lasso with Shift.**
4. **`selection` as data**, with `index` on each point as well.
5. **No selection across linked charts** in this RFC.

## Not in this RFC

- **Cross-filtering** (a selection in one chart filtering the others'
  data): the app's, through `bind:selection`.
- **Brushing pies, maps, heatmaps, trees.**
- **Saving a selection** (`persist`): a selection is a moment's, a window
  is a view's.

## Alternatives considered

- **ECharts' `connect`**: shared by index, which lines up only identical
  axes, and gives the app nothing to read.
- **Leave linking to `bind:range`**: works for the window, but the pointer
  can't be shared that way, and every dashboard writes the same effects.
- **Selection by clicks only** (`onselect` with Shift for more): fine for a
  few points, not for "these forty, up here".
