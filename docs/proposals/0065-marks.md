# RFC 0065 — Marks: a target, a threshold, a period

**Status:** implemented (2026-11-28) · **Target:** 0.63.0 · builds on [RFC 0058](0058-elyra-charts.md), [RFC 0061](0061-zoom.md) and [RFC 0063](0063-linked-charts.md) · see [Decisions](#decisions)

## Summary

Most of the charts a business app draws are measured against something:
revenue against the budget, response times against the SLA, a temperature
against its normal range, the weeks of a campaign. Today `@elyra/charts`'
five charts can only draw the data; the budget is a second series of the
same value in every month (a legend entry, a tooltip line and a summary
sentence that are noise), and a period can't be drawn at all. WireCharts'
`LineAnnotatedChart` does it for one kind of chart, with its own option.

This RFC adds `marks` to `LineChart`, `AreaChart`, `ColumnChart`,
`BarChart` and `ScatterChart`:

```svelte
<ColumnChart
  series={revenue}
  categories={months}
  marks={[
    { value: 120_000, label: "Budget" },
    { value: [80, 120], label: "Normal", tone: "success" },
    { at: "2026-03", label: "Price change" },
    { at: ["2026-06", "2026-08"], label: "Campaign" },
  ]}
/>
```

- **`value`** is on the value axis: one number is a line (the budget), two
  are a band (the normal range).
- **`at`** is on the category or time axis (x on a scatter): one is a line
  (a day something happened), two are a band (a period).
- **`label`** is drawn by the mark and said; **`tone`** picks its colour
  from the tokens (`muted` by default, `accent`, `success`, `warning`,
  `danger`).
- **What it says**: the summary names the marks, and a value line says how
  many points are over it: "Budget 120 000: 4 of 12 months over".

## Motivation

- **A number means something against a target.** "Revenue 112 000 in
  May" is half the answer; "under budget" is the other half, and the chart
  is where it's seen.
- **A target as a series is wrong in every channel.** It's in the legend
  as if it were data, in each tooltip, in the summary ("Budget from
  120 000 to 120 000"), in the CSV, and zoomed and selected as if it were
  points.
- **Periods explain the data.** A spike in March is a price change; a
  band over the campaign's weeks says so without a paragraph beside the
  chart.
- **A screen reader user needs the comparison too.** A line drawn on a
  canvas says nothing; "4 of 12 months over budget" is what the line is
  for.

## Current state (0.62)

- The five charts draw `series` only; `custom` options (`Chart`) can carry
  ECharts' `markLine` and `markArea`, unstyled and unsaid.
- WireCharts' `LineAnnotatedChart`, `LineForecastChart` and
  `SplineBandsChart` draw marks of their own kinds, with WireCharts'
  options.
- ECharts has `markLine` (by `xAxis`/`yAxis` value) and `markArea`
  (a pair of corners) on a series; a value outside the axis's extent isn't
  drawn, since marks don't widen the axis.

## Design

### `marks`

An array; each mark is one of:

| Mark | Drawn as |
|---|---|
| `{ value: n }` | a dashed line across, at `n` on the value axis |
| `{ value: [a, b] }` | a band between `a` and `b` on the value axis |
| `{ at: c }` | a dashed line along, at category (or date, or x) `c` |
| `{ at: [c, d] }` | a band from `c` to `d` |

with `label` (optional, drawn at the line's end or the band's top) and
`tone` (`muted`, `accent`, `success`, `warning`, `danger`). In a
`BarChart` the axes are turned, and so are the marks: `value` is upright.
A category `at` that isn't in `categories` is snapped to the nearest one
(RFC 0063's `nearestIndex`); on a time axis it's a date, as the
categories are.

### Drawing

The marks are carried by a series of their own: an empty line series
with `markLine` and `markArea`, left out of the legend, the tooltip, the
summary, the table, the CSV, the brush and the keys. So hiding the first
series in the legend doesn't hide the budget, and a stacked chart's stack
isn't touched. Lines are 1 px, dashed, in the tone's colour; bands are the
tone at 12 % (as RFC 0063's brush, `withAlpha`), behind the data. Labels
are in the tokens' small text, in the tone.

The value axis is widened to take in each `value` mark (ECharts'
`min`/`max` as functions of the data's extent), so a budget above every
month is still on the chart. A percent stack (`percent`) takes `value` in
percent.

### With zoom, links and print

- **Zoom** (RFC 0061): an `at` mark outside the window isn't drawn, and
  the window's summary names only the marks in it.
- **Links** (RFC 0063): marks are each chart's own; linked charts don't
  share them.
- **Print and PDF** (RFC 0062): the tones resolve to the light theme's on
  paper, as the rest of the chart does.
- **Export** (RFC 0060): the image has the marks; the CSV doesn't (it's
  the data).

### What it says

The summary adds a sentence per mark after the series':

- a value line: "Budget 120 000: 4 of 12 months over." (with one series;
  with several, "Budget 120 000.")
- a value band: "Normal 80 to 120: 9 of 12 in it."
- a category line: "Price change at March."
- a category band: "Campaign from June to August."

The hidden table gets the same sentences in a paragraph after it, and a
category band marks its rows (`aria-describedby` to the band's sentence).
The words are the framework's (`elyra.charts.markOver`, `markIn`,
`markAt`, `markSpan`), in English and Norwegian.

### Testing

- `marksOf`: each kind of mark into `markLine`/`markArea`, turned for
  `BarChart`, snapped categories, dates on a time axis, the axis widened,
  percent stacks.
- The marks' series out of the legend, tooltip, table, CSV, brush and keys.
- The summary's sentences: over and in counts, one series and several,
  in a zoomed window.
- A browser check in the gallery: a column chart against a budget and a
  line over a campaign.

## Implementation plan

1. `marksOf` in `internal/options.js`, for the cartesian charts and the
   scatter; the tones in `theme.js`; the axis widened.
2. The summary's and the table's sentences, the words.
3. The five components' `marks`; `Chart`'s pass-through; the gallery's
   Marks section; docs, CHANGELOG.

## Open questions

1. **`value`/`at`, or `y`/`x`?**
   *Recommendation:* `value`/`at`. They mean the same in a `BarChart`,
   where y and x are turned, and say what the app means ("the budget's
   value", "at March").
2. **Count the points over a value line in the summary?**
   *Recommendation:* yes, with one series (or a stack's total): it's the
   question the line is drawn to answer. With several separate series the
   counts would be a sentence per series; the summary names the line only.
3. **Widen the value axis to take in a mark?**
   *Recommendation:* yes. A budget off the chart is a mark the user never
   sees, and the app can't know the axis's extent to fix it.
4. **Marks on the pie, the heatmap and WireCharts' charts?**
   *Recommendation:* not in this RFC. A pie has no axis to mark; WireCharts'
   charts that mark have their own options.
5. **A mark's tooltip (hover the budget line to see its value)?**
   *Recommendation:* no: the label carries the value when the app puts it
   there ("Budget 120 000"), and a line a pointer must find is one more
   thing the keys can't reach.

## As built

- **`internal/marks.js`**: `marksFrom` reads the app's marks (a `value`
  that isn't a number, or a mark with neither, is dropped), `marksOf` makes
  the two series that carry them (`elyra-marks-areas` at `z: 1`, behind
  the data; `elyra-marks-lines` at `z: 3`, over it), empty, silent, out of
  the tooltip, and `marksSaid` and `bandRows` say them. The cartesian and
  scatter options append them; with marks, the legend's `data` is the
  data's series only.
- **The axis is widened by ECharts 6's `dataMin`/`dataMax`**, which join
  the data's extent before the axis is rounded, rather than `min`/`max`
  functions, which would fix the axis at the mark's value exactly.
- **A band over bars takes in the bars at its ends**: half a step out on
  each side (`[lo - 0.5, hi + 0.5]`), so "June to August" covers June's
  column and August's.
- **A category that isn't there** is the nearest one when the categories
  and the mark are dates, and isn't drawn otherwise.
- **`MarkLineComponent` and `MarkAreaComponent`** are in a chart's `use`
  only when it has marks.
- **`Chart` doesn't take `marks`**: its option is the app's, and draws
  ECharts' `markLine` and `markArea` as the app writes them.
- **The table**: a `<p>` a mark after it, and a period's rows point at its
  sentence by `aria-describedby`.
- **Checked in Chromium**: the gallery's Marks section (a column chart
  against a budget and the summer, a year of visits with a normal range, a
  campaign and a day, a bar chart's target beyond every bar), in both
  themes; zoomed into May, the campaign is drawn and said, September's day
  is neither.

## Decisions

1. **`value`/`at`**, not `y`/`x`.
2. **The points over a value line are counted** in the summary, with one
   series (or a stack's total); with several, the line is named only.
3. **The value axis is widened** to take in each `value` mark.
4. **No marks on the pie, the heatmap or WireCharts' charts** in this RFC.
5. **No tooltip on a mark**: its label carries what the app puts there.

## Not in this RFC

- **Marks the user drags** (moving a threshold to set it).
- **Per-point annotations** (a pin on one point with a note); `onselect`
  and the app's own popover do that today.
- **A target per category** (a budget that differs month by month): a
  series, drawn as a line over columns, is that; a later RFC may give it a
  style of its own.

## Alternatives considered

- **The target as a series**, as today: wrong in the legend, the tooltip,
  the summary, the CSV, the zoom and the brush.
- **ECharts' `markLine` on the first series**: hidden with that series in
  the legend, stacked with it, and unsaid.
- **`custom` options only**: every app styles and describes its own
  budget line, and none of them says it.
