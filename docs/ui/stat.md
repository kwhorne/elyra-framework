# Stat and Sparkline

A figure that matters, and where it's been. A Stat is a card with a label,
a value, how it changed and the numbers behind it. A Sparkline is a small
chart of a few numbers, in plain SVG: in a Stat, a Table's cell, a tray
popover. For axes, legends and tooltips, a chart library is the tool.

<!-- example: stat/kpis -->
```svelte
<script>
  import { Stat } from "@elyra/ui";

  const kroner = (v) => new Intl.NumberFormat("nb-NO", { style: "currency", currency: "NOK", maximumFractionDigits: 0 }).format(v);
</script>

<div class="kpis">
  <Stat label="Revenue" value={482000} format={kroner} change={0.12} caption="vs. last month" values={[310, 340, 330, 390, 420, 482]} />
  <Stat label="Open tickets" value={14} change={-0.25} good="down" caption="vs. last week" values={[22, 19, 20, 17, 14]} />
  <Stat label="Customers" value={1284} change={0} caption="vs. yesterday" />
</div>

<style>
  .kpis { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; width: 100%; }
</style>
```

`change` is a fraction (`0.12` is up 12 %), and `good` says which way is
good news: `up` for revenue, `down` for open tickets. The change is
coloured so, and a screen reader hears it said ("Up 12% vs. last month").
`format` shows the value as the app does (a currency, a unit); a number
is shown in the user's locale.

## Sparkline

`kind` is a `line`, a filled `area` or `bars`; `tone` colours it from the
theme's tokens, and `auto` is success when the last number is up on the
first and danger when it's down. What isn't a number is left out, so a gap
in the data isn't drawn as a zero. Without a `label` a screen reader hears
where the numbers went ("From 12 to 34, between 9 and 34").

<!-- example: stat/sparklines -->
```svelte
<script>
  import { Sparkline } from "@elyra/ui";

  const week = [12, 18, 9, 22, 30, 26, 34];
</script>

<div class="row">
  <Sparkline values={week} label="Orders this week" />
  <Sparkline values={week} kind="area" tone="success" />
  <Sparkline values={week} kind="bars" width={64} />
  <Sparkline values={[40, 38, 31, 27, 22]} tone="auto" />
</div>

<style>
  .row { display: flex; gap: 20px; align-items: center; }
</style>
```

`min` and `max` fix the scale, so several sparklines side by side can be
compared: without them each is scaled to its own numbers.

## API

### Stat

| Prop | Type | Default | Description |
|---|---|---|---|
| `label` | `string` | — | What the figure is. Names the card. |
| `value` | `number \| string` | — | The figure. A number is shown in the user's locale. |
| `change` | `number` | — | How it changed, as a fraction: `0.12` is up 12 %. |
| `good` | `"up" \| "down"` | `"up"` | Which way is good news, for the change's colour. |
| `caption` | `string` | — | Against what: "vs. last month". |
| `values` | `number[]` | — | The numbers behind it, drawn as a Sparkline. |
| `format` | `(value) => string` | — | How the value is shown. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | Under the figure. |

### Sparkline

| Prop | Type | Default | Description |
|---|---|---|---|
| `values` | `number[]` | `[]` | The numbers. What isn't a number is left out. |
| `kind` | `"line" \| "area" \| "bars"` | `"line"` | How they're drawn. |
| `tone` | `"accent" \| "success" \| "warning" \| "danger" \| "muted" \| "auto"` | `"accent"` | Its colour; `auto` by which way the numbers went. |
| `width` | `number` | `80` | In pixels. |
| `height` | `number` | `24` | In pixels. |
| `min` | `number` | — | The bottom of the scale. The lowest number without it. |
| `max` | `number` | — | The top of the scale. The highest number without it. |
| `label` | `string` | — | What a screen reader hears. Where the numbers went, without it. |
| `class` | `string` | `""` | |
