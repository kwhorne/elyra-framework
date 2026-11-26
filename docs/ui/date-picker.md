# DatePicker and DateRangePicker

A date, typed or picked in a calendar. The value is an ISO date
(`"2026-11-30"`) or `null`, whatever the user's locale, so it goes to
Rust and Laravel as it is. The field writes it the way the locale writes
dates in numbers (`30.11.2026`, `11/30/2026`), and takes that or ISO when
typed. It looks the same in WebKit and WebView2, unlike `<input
type="date">`.

<!-- example: date-picker/basic -->
```svelte
<script>
  import { DatePicker, Field } from "@elyra/ui";

  let due = $state("2026-11-30");
</script>

<!-- The value is ISO whatever the locale; the field writes it the user's way. -->
<Field label="Due" description="Type it, or pick it in the calendar.">
  <DatePicker bind:value={due} />
</Field>
<p class="value">value: {JSON.stringify(due)}</p>

<style>
  .value { margin: 8px 0 0; color: var(--elyra-muted); font: 12px var(--elyra-font-mono); }
</style>
```

- **Typing** takes three numbers in the locale's order, with any
  separator (`30.11.26`, `30/11/2026`), or ISO. A two-digit year is this
  century. What isn't a date, or is outside `min` and `max`, puts back
  the value when the field is left. Emptying the field clears it.
- **The calendar** opens from the button, or with ↓ in the field, on the
  value (or today). It starts the week where the locale does: Monday in
  Norway, Sunday in the US. `weekStart` sets it.
- **`min` and `max`** are ISO dates. Days outside them can't be picked,
  in the calendar or by typing.

## A range

A DateRangePicker is a span of days: `bind:start` and `bind:end`, each an
ISO date or `null`, so a filter can be open at one end ("since 1 March").
Both are typed as the locale writes dates; a start typed after the end
swaps them. The calendar shows two months: click (or Enter on) the first
day, then the last, the span shown between them as the pointer or the
focus moves. The months move on when the focus goes past the second.

<!-- example: date-picker/range -->
```svelte
<script>
  import { DateRangePicker, Field } from "@elyra/ui";

  let start = $state("2026-11-02");
  let end = $state("2026-11-15");
</script>

<div class="range">
  <Field label="Period"><DateRangePicker bind:start bind:end /></Field>
  <p>{start ?? "…"} → {end ?? "…"}</p>
</div>

<style>
  .range { display: grid; gap: 8px; max-width: 320px; }
  p { margin: 0; color: var(--elyra-muted); font: 12px var(--elyra-font-mono); }
</style>
```

`presets` are spans a click away, on the calendar's left: by default today,
the last 7 and 30 days, this month, last month and this year. Give your
own as `{ label, start, end }` (an end left out is open), or a function of
today, ISO, that returns them; `[]` for none. A preset is held in `min` and
`max`. `onchange` is called with `{ start, end }`.

## In a form

In a [Field](form.md), the field is labelled, described and marked invalid
by it, and `make:resource --view` uses a DatePicker for a model's date
fields.

## Accessibility

It follows WAI-ARIA's date picker dialog:

- The field is a labelled text input. The button is "Choose date", with
  the chosen date in full when there is one, and says it opens a dialog.
- The calendar is a modal dialog named by its month, with a grid of
  days. Each day's name is its full date, today is `aria-current="date"`,
  and the chosen one `aria-selected`. Tab stays inside it.
- In the grid: ←/→ a day, ↑/↓ a week, Home/End the week's first and last
  day, PageUp/PageDown a month, Shift+PageUp/PageDown a year. Enter or
  Space picks. Esc closes without picking. Focus goes back to the button.
- The month's name is a live region, so paging is read out.
- A DateRangePicker's two fields are "Start date" and "End date", in a
  group its Field names. Its days in the span are `aria-selected`, and a
  live line says whether the first or the last day is being picked.

## API

### DatePicker

| Prop | Type | Default | Description |
|---|---|---|---|
| `value` (bindable) | `string \| null` | `null` | The date, ISO (`"2026-11-30"`). |
| `min` | `string` | — | The first date that may be chosen, ISO. |
| `max` | `string` | — | The last date that may be chosen, ISO. |
| `locale` | `string` | — | How dates are written and named. The system's without it. |
| `weekStart` | `number` | — | The week's first day, `0` Sunday to `6` Saturday. The locale's without it. |
| `placeholder` | `string` | — | The field's hint. The locale's pattern (`DD.MM.YYYY`) without it. |
| `clearable` | `boolean` | `true` | A button that clears the date. |
| `disabled` | `boolean` | `false` | |
| `onchange` | `(value) => void` | — | After the date changes, with the ISO date or `null`. |
| `labels` | `object` | `{}` | Its words, for this instance: the app's translations otherwise ([its words](../i18n.md#the-frameworks-own-words)). |
| `class` | `string` | `""` | |

Other attributes go to the text input.

### DateRangePicker

| Prop | Type | Default | Description |
|---|---|---|---|
| `start` (bindable) | `string \| null` | `null` | The first day, ISO. |
| `end` (bindable) | `string \| null` | `null` | The last day, ISO. |
| `min` | `string` | — | The first date that may be chosen, ISO. |
| `max` | `string` | — | The last date that may be chosen, ISO. |
| `locale` | `string` | — | How dates are written and named. The system's without it. |
| `weekStart` | `number` | — | The week's first day, `0` Sunday to `6` Saturday. The locale's without it. |
| `presets` | `{ label, start?, end? }[] \| (today) => …` | `defaultPresets` | Spans a click away. `[]` for none. |
| `placeholder` | `string` | — | Each field's hint. The locale's pattern without it. |
| `clearable` | `boolean` | `true` | A button that clears both ends. |
| `disabled` | `boolean` | `false` | |
| `onchange` | `({ start, end }) => void` | — | After either end changes. |
| `labels` | `object` | `{}` | Its words, for this instance: the app's translations otherwise ([its words](../i18n.md#the-frameworks-own-words)). |
| `class` | `string` | `""` | |
