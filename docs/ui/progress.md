# Progress

How far along something is, or how full. A `progress` is an upload, a
sync, step 3 of 5; without a `value` it's under way for a time nobody
knows. A `meter` is a quota, disk space, seats used: it turns `warning`
past `low` and `danger` past `high`.

<!-- example: progress/bars -->
```svelte
<script>
  import { Progress } from "@elyra/ui";
</script>

<div class="stack">
  <Progress label="Uploading invoices.pdf" value={62} showValue />
  <Progress label="Syncing" />
  <Progress label="Step 3 of 5" value={3} max={5} size="sm" />
</div>

<style>
  .stack { display: grid; gap: 16px; width: 100%; max-width: 360px; }
</style>
```

`label` is what it measures: a screen reader hears it, with the value
(`aria-valuetext`), and `showValue` puts both above the bar. The value is
kept between 0 and `max`.

## Meters

Where more is worse (storage), `low` is below `high`. Where less is worse
(a battery), give `low` above `high` and the thresholds count the other
way. `format` says the value as the app does (`3 of 5 GB`).

<!-- example: progress/meters -->
```svelte
<script>
  import { Progress } from "@elyra/ui";

  const gb = (v, max) => `${v} of ${max} GB`;
</script>

<div class="stack">
  <Progress kind="meter" label="Storage" value={42} max={50} low={35} high={45} format={gb} showValue />
  <Progress kind="meter" label="Seats" value={18} max={25} low={20} high={24} showValue />
  <Progress kind="meter" label="Battery" value={9} low={30} high={10} showValue />
</div>

<style>
  .stack { display: grid; gap: 16px; width: 100%; max-width: 360px; }
</style>
```

An indeterminate bar stands still for a user who has asked for reduced
motion.

## API

### Progress

| Prop | Type | Default | Description |
|---|---|---|---|
| `value` | `number` | — | How far, of `max`. Without it, under way for an unknown time. |
| `max` | `number` | `100` | |
| `kind` | `"progress" \| "meter"` | `"progress"` | How far along (`progressbar`), or how full (`meter`). |
| `label` | `string` | — | What it measures, for a screen reader and `showValue`. |
| `showValue` | `boolean` | `false` | The label and value above the bar. |
| `low` | `number` | — | A meter is `warning` past it. |
| `high` | `number` | — | A meter is `danger` past it. Below `low` where less is worse. |
| `tone` | `"accent" \| "success" \| "warning" \| "danger"` | — | A colour of its own, over the thresholds. |
| `size` | `"sm" \| "md" \| "lg"` | `"md"` | 4, 6 or 10 pixels high. |
| `format` | `(value, max) => string` | — | How the value is said. A percentage without it. |
| `class` | `string` | `""` | |
