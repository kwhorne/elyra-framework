# RFC 0058 — `@elyra/charts`: charts that know the app

**Status:** accepted (2026-11-26) · **Target:** 0.59.0 · builds on [`@elyra/ui`](0012-elyra-ui.md), [live queries](0002-live-queries.md) and [the framework's own words](0056-the-frameworks-own-words.md) · see [Decisions](#decisions)

## Summary

`@elyra/ui` has a `Sparkline` and a `Stat`, and nothing bigger. An app
that needs a real chart (revenue by month, a pie of open cases, hours per
person) adds a chart library by hand and wires all of this itself:
- the theme, so the chart follows dark mode and the design tokens;
- a live query, so the chart updates when a record changes;
- loading, error and empty states;
- the locale's number formats;
- what a screen reader hears.

Every app does that differently.

This RFC adds a package of its own, `@elyra/charts`. It has Svelte
components on [ECharts](https://echarts.apache.org), tree-shaken, so an app
only ships the charts it uses. Their data has the same shape as
[WireCharts](https://wirecharts.io)' (`series` and `categories`), so a
Laravel endpoint that already feeds WireCharts feeds these unchanged.

```svelte
<script>
  import { LineChart, PieChart } from "@elyra/charts";
  import { live } from "./bindings";
</script>

<LineChart
  series={[{ name: "Revenue", data: [120, 180, 150, 260] }]}
  categories={["Jan", "Feb", "Mar", "Apr"]}
  smooth
/>

<!-- A live query: the chart redraws when a record changes, with
     a skeleton while it loads and a callout if it fails. -->
<PieChart live={live.cases_by_status()} donut />
```

- **The data contract is WireCharts'**:
  - `series` is `[{ name, data }]`. A plain list of numbers is one series.
  - `categories` is the x axis.
  - `smooth`, `stack`, `horizontal` and `legend` are plain props, the same
    in Blade and Svelte.
- **The first charts** are WireCharts' free ones: `LineChart`, `AreaChart`,
  `ColumnChart`, `BarChart`, `PieChart` (with `donut`) and `ScatterChart`.
  `<Chart option={…}>` takes any ECharts option for the rest.
- **The tokens**:
  - The charts follow the app's theme, including dark, light, more contrast
    and the Theme Studio.
  - Eight new series tokens, `--elyra-chart-1` to `--elyra-chart-8`, are
    checked for contrast against `--elyra-surface`.
- **Live queries**: `live` takes what `Table`'s `page` takes, a live store,
  a `Live` or a plain value. The chart moves to the new data instead of
  redrawing from scratch.
- **Accessibility and the assistant**:
  - Every chart has a summary as its label, like `Sparkline`'s.
  - Every chart also has its data as a visually hidden table. Screen
    readers read it, and so does the assistant, which sees the window
    (RFC 0013).
- **The app's language**: its words are `elyra.charts.*` (RFC 0056), and
  its numbers and dates are formatted in the app's locale.

## Motivation

- **Dogfood.** WireCharts is a sister project: charts for Laravel
  Livewire, on ECharts. An Elyra app with a Laravel half has endpoints that
  build WireCharts' series already, and the desktop should draw the same
  data without translating it.
- **A chart is where live queries show.** A table over a live query updates
  one row. A chart over one moves every bar. Nothing in the framework
  connects the two today.
- **The theme can't be borrowed.** ECharts takes colours as values, not
  CSS variables. Each app has to read the tokens and redraw on a theme
  change, and most don't, so the chart stays light in a dark window.
- **A canvas is silent.** A chart drawn to a canvas says nothing to a screen
  reader or to the assistant asked "what was revenue in March?".

## Current state (0.57)

- `@elyra/ui`: `Sparkline` (plain SVG: `line`, `area`, `bars`, tones, an
  `aria-label` summary) and `Stat` (a value, its change and a sparkline). No
  axes, legends, tooltips or more than one series.
- `Table` takes a live query's `{ value, error, loading }` and shows
  `Skeleton`, `Callout` and `Empty` for it. That's the pattern to follow.
- The tokens (`ui/src/tokens.css`) have accent, success, warning and danger,
  but no colours for a chart's series.
- The release has two tarballs, `elyra-runtime-X.tgz` and `elyra-ui-X.tgz`.

## Design

### The package

The package is `charts/` in the repository, and it ships as a third tarball,
`elyra-charts-X.tgz`, made the same way as `ui/`:
- It ships its sources, so a checkout needs no build step.
- `@elyra/ui` and `@elyra/runtime` are peers at the same version.
- `echarts` is a dependency with an exact version.

`rata new --charts` adds it, and `rata upgrade` (RFC 0057) moves it with
the others.

### The components

| Component | ECharts | Props beyond the shared ones |
|---|---|---|
| `LineChart` | `line` | `smooth`, `step`, `stack`, `area` |
| `AreaChart` | `line` + `areaStyle` | `smooth`, `stack`, `percent` |
| `ColumnChart` | `bar` | `stack`, `percent` |
| `BarChart` | `bar`, axes swapped | `stack`, `percent` |
| `PieChart` | `pie` | `donut`, `rose` |
| `ScatterChart` | `scatter` | `size` (a series' third value as the bubble size) |
| `Chart` | anything | `option`, merged over the theme |

The props every chart shares:
- the data: `series`, `categories` and `live`;
- the layout: `height` (default `320`), `legend`, `tooltip`, `label`;
- the formatting: `format` (a value as text) and `locale`;
- `renderer` (`svg`, or `canvas` for many points);
- the callbacks: `onselect` (`{ series, category, value }`, so a click on a
  bar can open the record), `labels` and `class`.

Each component imports only its own ECharts chart and the components it
needs (`echarts/core`, `echarts/charts`, `echarts/components`), so an app
that only uses `PieChart` doesn't ship the line chart's code.

### The data

The data is WireCharts' contract:
- **Series:** `series` is a list of `{ name, data, …extra }`. A bare list
  of numbers is one series without a name.
- **Points:** `data` holds numbers, or `{ name, value }` pairs for a pie,
  or `[x, y]` pairs for a scatter.
- **The x axis:** `categories` is a list of strings, or ISO dates, which
  then become a time axis.
- **Other options:** anything else on a series (`stack`, `yAxisIndex`,
  `itemStyle`) is passed to ECharts as it is.

`live` takes what `Table`'s `page` takes. The value is the series, or
`{ series, categories }`, which is what a chart command returns:

```rust
#[command(live, can = "reports.view")]
async fn revenue_by_month(ctx: Ctx) -> Result<ChartData> { … }
```

`elyra::charts::ChartData { series: Vec<Series>, categories: Vec<String> }`
is a small type in the framework, so a command and `rata codegen` agree on
the shape. A Laravel endpoint that returns the same arrays decodes into it.

### The theme

The chart builds an ECharts theme from the tokens it reads off its own
element, with `getComputedStyle`, so a token overridden on a section counts
too. The theme takes:
- the text, muted and border colours;
- `--elyra-surface` for the tooltip;
- `--elyra-font`;
- the eight chart tokens;
- the durations, with no animation under `prefers-reduced-motion`.

The chart rebuilds the theme when the theme changes, which means
`data-theme` on `<html>`, the OS's appearance, or the Theme Studio. It
keeps the data and the zoom when it does.

The eight `--elyra-chart-*` tokens come in light and dark. They're checked
for at least 3:1 contrast against `--elyra-surface`, the ratio WCAG
requires for graphics, by `tokens.test.js` and in the Theme Studio. The
order puts the most distinct colours first, so two series never look alike.

### States

- **Loading:** while `live` is loading with no value yet, a `Skeleton` of
  the chart's height.
- **Error:** a `Callout` with the error, and Try again when the store can
  run again.
- **No points:** `Empty`, with `elyra.charts.empty` ("No data").

### What it says

- The `aria-label` is a summary, as `Sparkline`'s is:
  - for a line: "Revenue: from 120 to 260, between 120 and 260, over 4 months";
  - for a pie: the largest slices and their shares.
- A visually hidden `<table>` has the data: categories as rows, series as
  columns, and each value in `format`. Screen readers can move through it.
  The assistant reads it as part of the window (RFC 0013), so "what was
  revenue in March?" is answered from the chart on screen.
- `onselect` is also there from the keyboard. The chart is one tab stop,
  ←/→ move along the categories with the tooltip showing, and Enter selects.

### The app's language

The words are under `elyra.charts.*` in `framework/lang/en.json` and
`nb.json`: No data, Try again, the summary's phrases and the table's
caption. Numbers and dates use `Intl` in the app's locale, unless a
`locale` prop is given.

### Testing

- Each component's option for given props, as data: snapshots of the option
  object, not of pixels.
- The tree-shaking: a build of an app that imports only `PieChart` contains
  no `LineChart` code. This is checked in CI against the built bundle.
- The theme rebuilt on a `data-theme` change; no animation under reduced
  motion.
- Live: a store's new value updates the option without a new ECharts
  instance; loading, error and empty show their states.
- The hidden table and the summary, in English and Norwegian.
- The `--elyra-chart-*` contrast in `tokens.test.js`.
- A browser check of each chart in the docs, in light and dark.

## Implementation plan

1. The `--elyra-chart-*` tokens and their contrast test, in `@elyra/ui`.
2. `charts/`: the shared base (theme, states, the hidden table, keys),
   then the six charts and `Chart`.
3. `elyra::charts::ChartData` and its codegen.
4. The tarball in the release scripts, `rata new --charts`, `rata upgrade`.
5. The reference (`docs/charts/*.md` with examples, like `@elyra/ui`'s),
   CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **A package of its own, or part of `@elyra/ui`?**
   *Recommendation:* its own. ECharts is several hundred kB even
   tree-shaken, so an app without charts shouldn't install it. The roadmap
   asked for this too.
2. **SVG or canvas by default?**
   *Recommendation:* SVG, because it's crisp at any scale, can be selected
   in the inspector, and is cheap for the hundreds of points a desktop app
   usually draws. `renderer="canvas"` is for the many thousands.
3. **Which charts first?**
   *Recommendation:* WireCharts' free six (line, area, column, bar, pie and
   scatter), and `Chart` with `option` for anything else ECharts draws.
   Heatmap, gauge, funnel and radar come later, if apps ask for them.
4. **`ChartData` in the framework, or only a TypeScript type?**
   *Recommendation:* in the framework, as a small Rust type. A live command
   returns it, `rata codegen` types it, and a Laravel endpoint decodes into
   it, so all three agree on the shape.
5. **Should the assistant read the chart from the hidden table, or from
   data the chart registers with the runtime?**
   *Recommendation:* the hidden table. One thing then serves both the
   screen reader and the assistant, and the assistant already reads the
   window.

## Not in this RFC

- **WireCharts' Pro charts** (3D, sankey, sunburst, …). `Chart` with
  `option` draws them, without a component of their own.
- **Exporting a chart** as PNG or SVG. It's worth doing through the file
  dialog, but separately.
- **Zoom and brush on a time axis.**
- **A Laravel helper that builds the series.** The contract is plain
  arrays, which WireCharts' users write already.

## Alternatives considered

- **Chart.js**: smaller, but canvas only. It has less of what a business app
  draws (stacked percents, a time axis with gaps, donuts with labels), and
  WireCharts' options are ECharts', so the two would drift apart.
- **Hand-written SVG, as `Sparkline` is**: right for a sparkline, but
  axes, ticks, tooltips and legends are most of what a chart library is.
- **Bundling ECharts whole**, about 1 MB: simpler, but every app pays for
  every chart.

## Decisions

1. **A package of its own**, `@elyra/charts`, a third tarball.
2. **SVG by default**; `renderer="canvas"` for many thousands of points.
3. **WireCharts' free six first** (line, area, column, bar, pie, scatter),
   and `Chart` with `option` for the rest.
4. **`ChartData` is a Rust type in the framework**, so a live command,
   `rata codegen` and a Laravel endpoint agree on the shape.
5. **The assistant reads the hidden table**, the same one the screen
   reader does.
