# RFC 0072 — Targets: a goal for each category

**Status:** implemented (2026-11-28) · **Target:** 0.64.0 · builds on [RFC 0058](0058-elyra-charts.md) and [RFC 0065](0065-marks.md) · see [Decisions](#decisions)

## Summary

RFC 0065's marks are one value for the whole chart: the budget is 120,000
every month. Real budgets aren't: December's is twice June's, a sales
target grows each quarter, a headcount plan steps up. RFC 0065 left "a
target per category" for later, and the way to draw one today is a second
series, which is wrong in the legend, the tooltip, the summary, the CSV,
the zoom and the brush, the problems marks were made to solve.

This RFC gives the cartesian charts a target per category:

```svelte
<ColumnChart
  series={[{ name: "Revenue", data: actual }]}
  categories={months}
  targets={{ label: "Budget", data: budget }}
/>
```

- **`targets`**: `{ label, data, tone?, lower? }`, `data` one value a
  category (`null` for none), as a series' is; `lower` for a target that's
  a ceiling (a cost).
- **Drawn as a goal, not as data**: across each column (or bar) a short,
  thick line at its target, the bullet chart's mark; on a line or an area,
  a dashed step line through the targets.
- **Said against the data**: the summary ("Budget: 7 of 12 months met"),
  the tooltip ("Revenue 112,000 · Budget 130,000 · −18,000"), and the
  hidden table's own column (the target and the difference, row by row).
- **Not data**: out of the legend, the brush and the keys' points; the
  value axis takes the targets in; the CSV has them as a column of their
  own, named by the label, since a target per category is a number per row.

## Motivation

- **A budget is per month.** One line for the year says nothing about a
  December that's short of its own budget but over June's.
- **"Met or not" is per category.** A summary that counts the months that
  met their own target answers the question the chart is drawn for.
- **The bullet chart is the business chart.** Stephen Few's design (a bar,
  its target as a tick across it) is how a dashboard shows actual against
  plan in the least room; it's missing from `@elyra/charts`.
- **A second series is wrong in every channel**, as RFC 0065 said of a
  constant target; a varying one is no different.

## Current state (0.63)

- `marks` (RFC 0065): `{ value }` and `{ at }`, one line or band each,
  drawn by series of their own, said in the summary.
- `ColumnChart`, `BarChart`, `LineChart`, `AreaChart` draw `series`;
  ECharts can draw a tick per category with a `custom` series or a
  `pictorialBar` (a thin rect symbol at each value).

## Design

### `targets`

`{ label, data, tone, lower }`: `label` names it (the legend doesn't, the tooltip,
the summary and the table do), `data` is aligned with `categories` (a
shorter list leaves the rest without), `tone` is RFC 0065's (`accent` by
default), and `lower` says it's met at or under rather than at or over. On a time axis, `data` may be `[date, value]` pairs, as a
series' may.

### Drawn

| Chart | The target |
|---|---|
| `ColumnChart` | a horizontal tick across each column, 70 % of its width, 3 px |
| `BarChart` | a vertical tick across each bar |
| `LineChart`, `AreaChart` | a dashed step line through the targets |

The ticks are a `pictorialBar` series (`symbol: "rect"`, sized to the
band), above the columns, silent; the step line a `line` series, `step:
"middle"`, dashed. Either is out of the legend, the brush and the keys'
walk, as RFC 0065's marks are; a stacked chart's ticks are against the
stack's total. The value axis takes the targets in (`dataMin`/`dataMax`,
as RFC 0065's).

### Said

- **The summary**, after the series': "Budget: 7 of 12 months met" (at or
  over; with `lower: true` on the target, at or under, for a cost), with
  one series or a stack's total; with several separate series, "Budget
  shown".
- **The tooltip** at a category: the target and the difference from the
  series (or the total), signed, in the chart's `format`.
- **The hidden table**: a column named by the label, and one for the
  difference ("Budget", "Difference"), so a screen reader walks actual
  and target together; zoomed in, the window's rows, as RFC 0061's are.
- **The CSV** (RFC 0060): the target's column beside the series'.

### Words

`elyra.charts.targetMet` (":label: :count of :total met"),
`targetShown` (":label shown"), `difference` ("Difference"), in English
and Norwegian.

### Testing

- The option: ticks on a column and a bar chart (turned), the step line,
  a stack's total, the axis widened, `null` targets, a time axis's pairs.
- Out of the legend, the brush and the keys.
- The summary (met, `lower`, several series, zoomed), the tooltip's
  difference, the table's columns, the CSV.
- The gallery: a column chart of a year against a monthly budget, and a
  bar chart's bullet form; checked in Chromium.

## Implementation plan

1. `internal/targets.js`: the series that draw the targets, and what's
   said, held by tests without a DOM.
2. The four components' `targets`; the tooltip; the table and the CSV.
3. The gallery, docs (`docs/charts.md`), CHANGELOG.

## Open questions

1. **A tick (bullet chart) on columns, or the step line on every chart?**
   *Recommendation:* the tick on columns and bars, the step line on lines
   and areas. A step line through columns is hard to read against their
   tops; a tick is where the eye already is.
2. **"Met" as at or over, with `lower` for costs?**
   *Recommendation:* yes: most targets are floors (revenue, sales), and a
   cost's ceiling is one flag away.
3. **The target in the CSV?**
   *Recommendation:* yes, as a column: unlike a constant mark, a target per
   category is data a spreadsheet wants beside the actuals.
4. **Several targets (a budget and last year)?**
   *Recommendation:* one in this RFC. Last year is a series; a second
   target is rare, and `targets` can become a list later without breaking.
5. **Targets on the pie, the scatter and WireCharts' charts?**
   *Recommendation:* no: a pie has no axis to measure along, a scatter's
   points aren't categories, and WireCharts' charts have their own.

## As built

- **`internal/targets.js`**: `targetsFrom`, `measuredOf`, `targetsOf`,
  `tickOf`, `targetsSaid`, `targetColumns` and `tooltipWithTargets`, plain
  functions.
- **The tick is a `custom` series** (`CustomChart`, in a chart's `use` only
  with `targets`) rather than a `pictorialBar`: its `renderItem` sizes the
  tick to the band (`api.size`), 70 % of it and at most 48 px, as a
  column is. Its id starts `elyra-marks-`, so the brush leaves it out as it
  does RFC 0065's marks.
- **Its colour is the text's by default**, edged in the surface's: in the
  accent, the columns' own colour, a tick inside a column was lost. A
  `tone` sets it.
- **The tooltip** is a formatter of the chart's own when it has targets:
  each series' line as ECharts draws it, then the difference, signed, in
  the chart's `format`. The option is given the chart's words for it.
- **The table** has the target's column and "Difference"; the CSV the
  target's only, as the data's neighbour.
- **Checked in Chromium**: the gallery's Targets section (a column chart
  against a monthly budget, a bar chart's quota, a line under a ceiling),
  its summaries ("Budget: 8 of 12 met"), its table, and the tooltip by the
  keys ("Difference -3,000").

## Decisions

1. **A tick across columns and bars**, a dashed step line on lines and
   areas.
2. **"Met" is at or over**; `lower` makes it at or under, for a cost.
3. **The target is a column in the CSV.**
4. **One target** a chart in this RFC.
5. **No targets on the pie, the scatter or WireCharts' charts.**

## Not in this RFC

- **Several targets on one chart.**
- **A band per category** (a range to land in, a normal per month).
- **Coloured columns by whether they met** (red under, green over): the
  tick and the summary say it, and colour alone says it to no one who
  can't see it.

## Alternatives considered

- **A second series, styled**: the problems RFC 0065 listed, plus a
  legend entry and a CSV column named as if it were measured.
- **RFC 0065's marks, one per category**: a dozen lines with a dozen
  labels, and nothing that says "7 of 12 met".
- **A `BulletChart` of its own**: the bullet is a column chart with
  targets; one prop on the charts apps already use is less to learn.
