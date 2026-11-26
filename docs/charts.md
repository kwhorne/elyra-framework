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
"@elyra/charts": "https://github.com/kwhorne/elyra-framework/releases/download/v0.58.0/elyra-charts-0.58.0.tgz"
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
  Enter selects, Esc hides it.
- **Its words** are `elyra.charts.*`, in English and Norwegian with the
  framework, translated like the rest of [its words](i18n.md#the-frameworks-own-words).

## Only what's drawn

Each component imports its own ECharts chart (`echarts/charts`) and the
components every chart uses (grid, tooltip, legend, aria): an app that draws
only pies ships no line chart. `@elyra/charts`' tests build such an app and
check.
