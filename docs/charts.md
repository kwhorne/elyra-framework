# Charts (`@elyra/charts`)

Charts on [ECharts](https://echarts.apache.org), as Svelte 5 components that
know the app ([RFC 0058](proposals/0058-elyra-charts.md)): its design tokens
in every theme, its live queries, its language, and what a screen reader and
the assistant need to read them.

```bash
rata new myapp --charts        # a new app with it
```

In an existing app, beside `@elyra/ui` and at the same release:

```json
"@elyra/charts": "https://github.com/kwhorne/elyra-framework/releases/download/v0.63.0/elyra-charts-0.63.0.tgz"
```

`rata upgrade` moves it with the others.

## The data

[WireCharts](https://wirecharts.io)' contract, so a Laravel endpoint that
feeds a WireCharts chart feeds these unchanged:

```svelte
<ColumnChart
  series={[
    { name: "Revenue", data: [120, 180, 150, 260] },
    { name: "Costs", data: [90, 110, 130, 140] },
  ]}
  categories={["Jan", "Feb", "Mar", "Apr"]}
/>
```

- `series`: `[{ name, data, …extra }]`, or a bare list of numbers for one
  series. Anything else on a series (`stack`, `yAxisIndex`, `itemStyle`)
  goes to ECharts as it is.
- `data`: numbers (`null` is a gap), `{ name, value }` for a pie's slices,
  `[x, y]` (or `[x, y, size]`) for a scatter.
- `categories`: the x axis. ISO dates (`2026-03-01`) make it a time axis.

From Rust, `elyra::charts::ChartData`, which `rata codegen` types for the
page:

```rust
use elyra::charts::ChartData;

#[command(live, can = "reports.view")]
async fn revenue_by_month(ctx: Ctx) -> Result<ChartData> {
    let months = /* … */;
    Ok(ChartData::new(["Jan", "Feb", "Mar"])
        .series("Revenue", [120.0, 180.0, 150.0])
        .series("Costs", [90.0, 110.0, 130.0]))
}
```

`ChartData::slices([("Open", 12.0), ("Closed", 30.0)])` is a pie's;
`.points("Deals", [vec![10.0, 20.0, 4.0]])` a scatter's.

## Live

`live` takes what `Table`'s `page` takes: a live query's store, a `Live`, or
a plain value. The value is the series, or `{ series, categories }`.

```svelte
<ColumnChart live={live.revenue_by_month()} />
```

While it loads with nothing yet, a skeleton of the chart's height; if it
fails, a callout with the error; with no points, "No data". New data moves
the chart to it: the bars grow, nothing is drawn from scratch.

## The charts

| Component | Draws | Its own props |
|---|---|---|
| `LineChart` | lines | `smooth`, `step`, `stack`, `area` |
| `AreaChart` | lines filled to the axis | `smooth`, `stack`, `percent` |
| `ColumnChart` | upright bars, a group per category | `stack`, `percent` |
| `BarChart` | bars across, the categories down the side | `stack`, `percent` |
| `PieChart` | shares of a whole | `donut`, `rose` |
| `ScatterChart` | points on two value axes | `size` (a third value as the bubble's size) |
| `Chart` | anything ECharts draws | `option`, `use` |

Every chart also takes:

| Prop | Default | |
|---|---|---|
| `series`, `categories`, `live` | `[]`, `[]`, — | The data, above. |
| `height` | `320` | In pixels, or any CSS length. The width is its container's. |
| `legend`, `tooltip` | `true` | The legend shows with more than one named series. |
| `label` | — | What a screen reader hears; without one, a summary of the data. |
| `format` | — | A value as text (`(v) => `${v} kr``); else the locale's numbers. |
| `locale` | the app's | For numbers and dates. |
| `renderer` | `"svg"` | `"canvas"` for many thousands of points. |
| `onselect` | — | Told `{ series, category, value }` on a click, or Enter. |
| `labels` | `{}` | Its words, for this instance ([translations](i18n.md#the-frameworks-own-words)). |
| `exportable` | `true` | The menu that saves or copies it ([taking it out](#taking-it-out)). |
| `link`, `brush`, `selection` | —, `false`, `null` | Linked charts and a selection ([linked](#linked-charts-and-a-selection)); on the five above. |
| `marks` | — | A target, a threshold or a period, drawn and said ([marks](#marks)); on the five above. |
| `targets` | — | A goal for each category, drawn as a tick or a step line and said as met or not ([targets](#targets)); on line, area, column and bar. |
| `zoom`, `range`, `persist` | `false`, `null`, — | A window on a long series ([zoom](#zoom)); not on `PieChart`, and on `Chart` when its option has a `dataZoom`. |
| `class` | `""` | |

`Chart` draws an option of the app's own, over the theme's colours and
text. `use` is the ECharts parts the option needs, so only those are in the
app:

```svelte
<script>
  import { Chart } from "@elyra/charts";
  import { HeatmapChart } from "echarts/charts";
  import { VisualMapComponent } from "echarts/components";
</script>

<Chart use={[HeatmapChart, VisualMapComponent]} option={{ series: [{ type: "heatmap", data }], … }} label="Load by hour" />
```

## Every WireCharts chart

Beyond these, every one of [WireCharts](https://wirecharts.io)' 72 charts is
a component with WireCharts' props ([RFC 0059](proposals/0059-all-of-wirecharts.md)):
the lines and areas (`SplineChart`, `LineForecastChart`, `AreaFanChart`,
`StreamgraphChart`, …), the bars (`ColumnStackedChart`, `ParetoChart`,
`HistogramChart`, …), the combinations, the pies (`DonutChart`,
`PieSemiChart`, …), `FunnelChart`, `GaugeChart`, `ClockChart`,
`HeatmapChart`, `CandlestickChart`, `BoxplotChart`, `RadarChart`,
`TreeChart`, `TreemapChart`, `SunburstChart`, `SankeyChart`, `GraphChart`
and `AudioChart`, which plays its series as sound. They're listed, with an
example each, in [every chart](charts-reference.md), and drawn in the
gallery: `npm run docs` in `charts/` shows every one with its code, in dark
or light, in English or Norwegian.

```svelte
<script>
  import { FunnelChart, SankeyChart } from "@elyra/charts";
</script>

<FunnelChart series={[100, 80, 60, 30]} labels={["Visits", "Signups", "Trials", "Paid"]} />
<SankeyChart live={live.flows()} />
```

- **The props are WireCharts'**, so a Blade view's `<chart:funnel>` and a
  page's `<FunnelChart>` say the same. Each one's option is WireCharts' own,
  ported to JavaScript and held to it by tests, in the app's look.
- **`live`** takes the props as an object (`{ nodes, links }` for a sankey),
  or a list as the series.
- **The ones that move**: `ClockChart` ticks (not while the window is
  hidden), `LineRaceChart` and `AreaRaceChart` draw their points one by one
  with a Replay button, and `AudioChart` has a button that plays the series
  as pitch. Under reduced motion the races show their whole line.
- **3D** (`Bar3dChart`, `Scatter3dChart`, `SurfaceChart`) comes from
  `@elyra/charts/gl` and needs `echarts-gl` beside it; without WebGL the
  chart says so, and keeps its summary and table.

## Taking it out

Right-click a chart, or press Shift+F10 or the Menu key on it, for a menu in
the app's language ([RFC 0060](proposals/0060-exporting-a-chart.md)):

- **Copy as image**: a PNG on the clipboard, to paste into Word, Keynote or
  Mail.
- **Save as PNG… / Save as SVG…**: where the user chooses, named from the
  chart's `label` (or its summary) and the date: `Revenue 2026-11-27.png`.
  The PNG is twice the chart's pixels; both have the theme's surface behind
  them, so a dark chart reads on a white page. A canvas or 3D chart offers
  no SVG.
- **Copy data / Save data as CSV…**: the chart's table, its caption first,
  with plain numbers in the app's locale and the separator its spreadsheet
  reads: `1200,5` and semicolons in Norwegian, `1200.5` and commas in
  English.

Without the `system` feature (no save dialog, no image clipboard), the menu
offers only Copy data. `exportable={false}` turns it off. An app's own button
calls the chart's methods:

```svelte
<script>
  let chart;
</script>

<LineChart bind:this={chart} {series} {categories} label="Revenue" />
<Button onclick={() => chart.save("png")}>Export</Button>
<!-- chart.save("svg" | "csv"), chart.copy(), chart.csv() -->
```

## Zoom

A year of days drawn whole is a smudge. `zoom` gives a window on it
([RFC 0061](proposals/0061-zoom.md)), on `LineChart`, `AreaChart`,
`ColumnChart`, `BarChart` and `ScatterChart` (and on WireCharts'
`LineTimeChart`, `SplineTimeChart` and `LineBoostChart`, where it's on by
default, and on `Chart` when its option has a `dataZoom`):

```svelte
<LineChart {series} categories={days} zoom bind:range persist="sales" />

<p>{range ? `${range[0]} – ${range[1]}` : "All of it"}</p>
<Button onclick={() => (range = ["2026-03-01", "2026-03-31"])}>March</Button>
<Button onclick={() => (range = null)}>All</Button>
```

- **The pointer**: the wheel or a pinch zooms, a drag pans, and a slider
  under the chart (beside it, for `BarChart`) shows where the window is.
  Off by default: a wheel over a dashboard scrolls the page.
- **`bind:range`** is the window as the axis's own values: dates as they
  were given, categories, or a scatter's x, and `null` for all of it. Set
  it to jump; read it to filter a table beside the chart.
- **`persist="key"`** keeps the window in the app's settings store, and
  brings it back.
- **The keyboard**: on the chart, + and − zoom about the focused category,
  Shift+←/→ pan, 0 shows all, and ←/→ move the window when they walk past
  its edge. The slider's two handles are in the tab order, as sliders named
  "Start of the window" and "End of the window" with their dates as values;
  ←/→ move them a category, Page Up/Down a tenth, Home/End to the ends.
- **What it says** follows the window: "Mar 1 – Mar 31: Revenue: from 120
  to 260…". The hidden table keeps every row, so the assistant can answer
  about any month, and marks the shown ones.
- **Live data** keeps the window on its values; a window at the axis's end
  follows the end, so "the last 30 days" stay the last 30.
- **Export** saves what's drawn as the image, and every row as CSV.

## Linked charts and a selection

Charts over the same time are linked by a name, and a chart with `brush`
lets the user select ([RFC 0063](proposals/0063-linked-charts.md)):

```svelte
<LineChart series={revenue} categories={days} zoom link="sales" />
<ColumnChart series={orders} categories={days} zoom link="sales" />

<ScatterChart series={customers} brush bind:selection />
<Table rows={selection ? selection.points.map((p) => rows[p.index]) : rows} … />
```

- **`link="name"`**: the page's charts with the same name share their
  window (zoom one, the others follow) and their pointer (point at the
  14th in one, each shows its tooltip at the 14th). By the axis's values,
  not by position: a value one chart lacks is the nearest it has, so a
  month of days and a year of weeks stay together. With `bind:range`, the
  range is the group's. A linked chart says so to a screen reader ("Linked
  with 2 other charts").
- **`brush`**: drag a rectangle over a scatter (a lasso with Shift held),
  or a span along a category or time axis. A drag brushes; with `zoom`,
  the wheel still zooms. The brush is in the accent colour, and what's
  outside it is dimmed.
- **`bind:selection`** is what's selected, as data: `{ points: [{ series,
  x, y, size?, seriesIndex, index }] }` over a scatter, `{ from, to,
  categories }` along an axis, `null` for nothing. Set it to draw it (a
  scatter's points as the rectangle around them).
- **The keys**: on the chart, Space starts a span at the focused category,
  ←/→ grow it, Space ends it; over a scatter, ←/→ walk the points and
  Space takes each in or out. Esc clears. Each change is said: "Mar 3 –
  Mar 14 selected (12)".
- **What it says**: the summary ends with the selection ("12 of 340
  selected"), and the hidden table's selected rows say "selected".

On the five of 0.58 (`LineChart`, `AreaChart`, `ColumnChart`, `BarChart`,
`ScatterChart`).

## Marks

A chart measured against something draws it with `marks`: a budget, a
normal range, a day something changed, a campaign's weeks
([RFC 0065](proposals/0065-marks.md)):

```svelte
<ColumnChart
  series={revenue}
  categories={months}
  marks={[
    { value: 120_000, label: "Budget" },
    { value: [80_000, 140_000], label: "Normal", tone: "success" },
    { at: "Mar", label: "Price change" },
    { at: ["Jun", "Aug"], label: "Summer", tone: "warning" },
  ]}
/>
```

| Mark | Drawn as |
|---|---|
| `{ value: n }` | a dashed line across, at `n` on the value axis |
| `{ value: [a, b] }` | a band from `a` to `b` on the value axis |
| `{ at: c }` | a dashed line at category (or date, or a scatter's x) `c` |
| `{ at: [c, d] }` | a band from `c` to `d`, the bars at its ends taken in |

- **`label`** is drawn by the mark and said; **`tone`** is its colour from
  the tokens: `muted` (the default), `accent`, `success`, `warning`,
  `danger`. Bands are the tone at 12 %, behind the data; lines are over it.
- **In a `BarChart`** the axes are turned, and so are the marks: `value` is
  upright. Over a scatter, `value` is y and `at` is x.
- **The value axis takes in each `value` mark**, so a budget above every
  month is still on the chart. In a percent stack, `value` is in percent.
- **A date** on a category axis that isn't one of the categories is the
  nearest one; a category that isn't there (and isn't a date) isn't drawn.
- **The marks aren't data**: they're not in the legend, the tooltip, the
  table's columns, the CSV, the brush or the keys, and hiding a series in
  the legend leaves them. The image (`save("png")`) has them.
- **What it says**: the summary ends with a sentence a mark, and the hidden
  table has them after it: "Budget 120,000: 6 of 12 over", "Normal 80,000
  to 140,000: 9 of 12 within" (with one series, or a stack's totals; with
  several, the line is named only), "Price change at Mar", "Summer from Jun
  to Aug". The rows a period covers point at its sentence. Zoomed in, only
  what's shown is counted, and a mark outside the window isn't said.

On the five of 0.58, as the selection is. `Chart`'s own option draws
ECharts' `markLine` and `markArea` as it gives them.

## Targets

A goal for each category, where `marks` has one for the whole chart: a
budget a month, a quota a product ([RFC 0072](proposals/0072-targets.md)):

```svelte
<ColumnChart
  series={[{ name: "Revenue", data: actual }]}
  categories={months}
  targets={{ label: "Budget", data: budget }}
/>
<LineChart series={costs} categories={months} targets={{ label: "Ceiling", data: ceiling, lower: true }} />
```

- **`targets`** is `{ label, data, tone?, lower? }`: `data` one value a
  category, as a series' (`null` for none; `[date, value]` on a time
  axis); `tone` from the tokens (the text's colour by default); `lower`
  for a ceiling, a cost's, met at or under rather than at or over.
- **Drawn as a goal**: across each column, or up each bar, a 3 px tick at
  its target, 70 % of the band (the bullet chart), edged so it reads
  inside a column and above it; on a line or an area, a dashed step line.
  A stack is measured by its total. The value axis takes the targets in.
- **Not data**: not in the legend, the brush or the keys' walk. A percent
  stack has none.
- **Said**: the summary ends with "Budget: 8 of 12 met" (with one series or
  a stack; "Budget shown" with several separate ones), over what's shown
  when zoomed in; the tooltip has the target and the difference
  ("Difference −3,000"); the hidden table has a column for the target and
  one for the difference.
- **Taken out**: the CSV has the target as a column beside the data.

## The look

The colours come from the design tokens, read off the chart's own element
(an override on a section counts), and again whenever the theme changes:
`data-theme`, more contrast, the OS's appearance, or the Theme Studio. The
series take `--elyra-chart-1` to `--elyra-chart-8`, each 3:1 or more against
every surface, as WCAG asks of graphics ([tokens](ui/tokens.md#charts)).
Under reduced motion nothing animates.

## What it says

- **Its name** is a summary: "Revenue: from 120 to 260, between 120 and 260,
  over 4 points"; a pie's, its largest slices and their shares.
- **Its data** is a table, seen by no one and read by the screen reader, and
  by [the assistant](assistant.md), which sees the window: "what was revenue
  in March?" is answered from the chart on screen.
- **The keyboard**: one tab stop; ←/→ walk the categories with the tooltip,
  Enter selects, Esc hides it. With `zoom`, + − 0 and Shift+←/→ move the
  window ([zoom](#zoom)).
- **Its words** are `elyra.charts.*`, in English and Norwegian with the
  framework, translated like the rest of [its words](i18n.md#the-frameworks-own-words).

## Only what's drawn

Each component imports its own ECharts chart (`echarts/charts`) and the
components every chart uses (grid, tooltip, legend, aria): an app that draws
only pies ships no line chart. `@elyra/charts`' tests build such an app and
check.
