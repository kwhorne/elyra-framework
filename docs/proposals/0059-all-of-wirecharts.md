# RFC 0059 — All of WireCharts' charts in `@elyra/charts`

**Status:** draft (2026-11-27) · **Target:** 0.59.0 · builds on [RFC 0058](0058-elyra-charts.md) · see [Open questions](#open-questions)

## Summary

`@elyra/charts` shipped in 0.58.0 with WireCharts' six basic charts. WireCharts
has 72. A desktop app with a Laravel half that draws a funnel, a heatmap or
a sankey on the web has to fall back to `<Chart option={…}>` on the
desktop, and write by hand what WireCharts already builds from a few props.

This RFC brings every one of them, as a component of its own with
WireCharts' props:

```svelte
<script>
  import { FunnelChart, HeatmapChart, SankeyChart, GaugeChart } from "@elyra/charts";
</script>

<FunnelChart series={[60, 40, 20]} labels={["Visits", "Sign-ups", "Orders"]} />
<GaugeChart value={72} max={100} name="Load" progress />
<SankeyChart {nodes} {links} live={live.flows()} />
```

- **One component per chart**: `area-fan` is `AreaFanChart`,
  `combo-dual-axis` is `ComboDualAxisChart`, and so on. Each imports only
  its own part of ECharts, so an app still ships only what it draws.
- **WireCharts' props, as they are**, so a Blade view and a Svelte page say
  the same thing. The six from 0.58 keep the props they have, and take
  WireCharts' names too (`labels` for a pie's slices).
- **The options are built in JavaScript**, ported from `Option.php`
  builder by builder. Formatters are real functions, so nothing is
  `eval`ed: the app's content security policy stays as it is.
- **What 0.58 gave the six, all 72 get**: the design tokens, `live` and its
  states, the summary and the hidden table, the keys and the app's language.
- **The charts that move** (`ClockChart`, `LineRaceChart`, `AreaRaceChart`,
  `AudioChart`) bring their behaviour too, and stop under reduced motion.
- **3D** (`Bar3dChart`, `Scatter3dChart`, `SurfaceChart`) comes from a separate
  entry, `@elyra/charts/gl`, because it needs `echarts-gl` and WebGL.

## Motivation

- **One chart vocabulary for both halves.** The web and the desktop of the
  same app should draw the same report the same way, from the same data and
  the same props.
- **`Chart` with `option` is a way out, not a way in.** A funnel is ten
  props in WireCharts and sixty lines of ECharts option by hand, and the
  theme, the summary and the table are the app's to get right.
- **The tests are already written, in PHP.** WireCharts' builders are the
  reference: the port can be held to them option by option.

## Current state (0.58)

- `@elyra/charts`: `LineChart`, `AreaChart`, `ColumnChart`, `BarChart`,
  `PieChart`, `ScatterChart` and `Chart`. `ChartBase` does the instance,
  the theme, the states, the summary, the table and the keys;
  `internal/options.js` builds the options as plain data.
- WireCharts: 72 Blade components over `Option.php`'s 66 builders, and
  Alpine glue for the four that move. Its PHP builders mark formatters as
  `"@@function …@@"` strings and revive them with `new Function`.

## Design

### The components

Every WireCharts component becomes `<Name>Chart`, PascalCase:

| Family | Components |
|---|---|
| Line | `Line`, `LineLabels`, `LineLog`, `LineTime`, `LineAnnotated`, `LineForecast`, `LineBoost`, `LineAnimated`, `LineRace`, `Spline`, `SplineInverted`, `SplineTime`, `SplineBands`, `Streamgraph` |
| Area | `Area`, `Areaspline`, `AreaStacked`, `AreaPercent`, `AreaInverted`, `AreaStackedInverted`, `AreaNegative`, `AreaMissing`, `AreaGradient`, `AreaRange`, `AreaRangeLine`, `AreaFan`, `AreaRace` |
| Column and bar | `Column`, `ColumnStacked`, `ColumnPercent`, `ColumnNegative`, `ColumnRotated`, `ColumnRange`, `Bar`, `BarStacked`, `BarPercent`, `BarRange`, `Lollipop`, `Histogram`, `Pareto` |
| Pie | `Pie`, `Donut`, `PieRose`, `PieSemi`, `PieVariable`, `PieGradient`, `PieLabels`, `PieMonochrome` |
| Scatter | `Scatter`, `Bubble`, `ScatterSymbols`, `ScatterRegression`, `PackedBubble` |
| Combinations | `ComboLineColumn`, `ComboDualAxis`, `ComboMultiAxis` |
| More | `Gauge`, `Clock`, `Radar`, `Funnel`, `Heatmap`, `Candlestick`, `Boxplot`, `Tree`, `Treemap`, `Sunburst`, `Sankey`, `Graph`, `Audio` |
| 3D (`@elyra/charts/gl`) | `Bar3d`, `Scatter3d`, `Surface` |

The 0.58 names stay. `AreaChart` and `ColumnChart` keep `stack` and
`percent`, the same charts as `AreaStacked` and `ColumnPercent` with a prop.

### The options

`internal/options/` holds one module per family, each exporting the builders
of `Option.php` under the same names (`areaFan`, `comboDualAxis`, …). They
take the same arguments and return the same option, with these changes:
- the theme's colours and text in place of WireCharts' theme JSON;
- functions in place of `"@@…@@"` strings;
- the app's locale in the formatters.

A builder that needs data shaped differently (a tree's nested nodes, a
sankey's nodes and links, a candlestick's OHLC) takes it as WireCharts does.
`ChartData` grows the shapes it doesn't have yet: `nodes`, `links`, and a
tree.

### Held to WireCharts

`scripts/wirecharts-fixtures.php` runs each `Option.php` builder on a fixture
of props and writes the options as JSON, with every function as
`"@@function@@"`. It's checked in with the WireCharts commit it came from.
The JS tests build the same fixtures and compare. Colours and functions are
left out of the comparison, because the theme and the locale are Elyra's.
When WireCharts changes, the script runs again and the diff shows what to
port.

### What moves

- **`ClockChart`** ticks every second, and stops when it's off screen or
  the window is hidden.
- **`LineRaceChart` and `AreaRaceChart`** reveal their points one by one,
  once.
- **`AudioChart`** plays the series as pitch on a button press: Web Audio,
  with the tooltip following the sound.
- Under reduced motion, the clock shows the time without its sweep, and the
  race charts show the whole line at once.

### 3D

`@elyra/charts/gl` exports the three 3D charts and imports `echarts-gl`, which
is an optional peer dependency the app installs only if it imports that
entry. Without WebGL the 3D charts show `Callout` ("3D isn't available
here") and still have their summary and table.

### Accessibility, the theme and the words

As RFC 0058. Each family's summary says what its chart says:
- a funnel: its stages, with how many are left at each;
- a gauge: its value out of its range;
- a sankey: its largest flows;
- a tree: its depth and breadth;
- a candlestick: open, high, low and close over the period.

The hidden table has the rows each family's data is made of. The words for
these are new `elyra.charts.*` keys, in English and Norwegian.

### Testing

- Each builder against WireCharts' fixture (above).
- Each component: draws with its own ECharts part, and a bundle with only it
  has no other chart's code (the 0.58 test, run per family).
- The four that move, with fake timers; reduced motion.
- `@elyra/charts/gl`: imported only when asked; the WebGL fallback.
- A browser check of every chart, in dark and light, in the docs.

## Implementation plan

1. The fixture script and the fixtures; `options/` split by family.
2. The families one by one, with their components, summaries and tables:
   line and area, column and bar, pie, scatter and combinations, the rest.
3. The four that move.
4. `@elyra/charts/gl`.
5. The docs: a page per family with every chart's example, like
   `@elyra/ui`'s reference; CHANGELOG.

## Open questions

1. **WireCharts Pro's charts become free here.** In WireCharts, 66 of the 72
   need a Pro licence. `@elyra/charts` is MIT, like the rest of Elyra, so
   in an Elyra app they'd be free.
   *Recommendation:* that's your call as WireCharts' owner. If they should
   stay paid, the alternative is that `@elyra/charts` ships the free six
   and a Pro package is installed beside it with a licence key. That's
   more work, and it doesn't fit "the repository is the only source".
2. **3D in a separate entry, with `echarts-gl` as an optional peer?**
   *Recommendation:* yes. `echarts-gl` is large and needs WebGL, and only an
   app that draws in 3D should carry it.
3. **One component per chart, or one `<Chart kind="area-fan">`?**
   *Recommendation:* one per chart. It's how WireCharts reads, and it keeps
   the tree-shaking. A `kind` prop would pull every builder in.
4. **Do the 0.58 six take WireCharts' prop names too** (`labels` beside
   `categories` for a pie)?
   *Recommendation:* yes, as aliases, so a Blade view's props work on the
   desktop as they are.
5. **Held to WireCharts by fixtures from PHP, checked in?**
   *Recommendation:* yes. CI then needs no PHP for the charts, and a change
   in WireCharts shows up when the fixtures are made again.

## Not in this RFC

- **WireCharts' licence check** (`wirecharts:activate`).
- **Maps** (ECharts' geo), which WireCharts doesn't have either.
- **Exporting a chart** as PNG or SVG (RFC 0058 left it out too).

## Alternatives considered

- **Running WireCharts' PHP builders from Laravel and sending the option**:
  the desktop would draw what the server builds, but only with the server,
  never offline, and with functions as strings revived by `new Function`.
- **`<Chart option>` and a cookbook**: no new code, but every app writes the
  same sixty lines and gets the theme and the table wrong in its own way.
