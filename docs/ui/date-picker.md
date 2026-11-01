# DatePicker

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

<!-- example: date-picker/range -->
```svelte
<script>
  import { DatePicker, Field } from "@elyra/ui";

  let from = $state("2026-11-02");
  let to = $state(null);
</script>

<!-- `min` and `max` bound each other: the end can't come before the start. -->
<div class="range">
  <Field label="From"><DatePicker bind:value={from} max={to ?? undefined} /></Field>
  <Field label="To"><DatePicker bind:value={to} min={from ?? undefined} /></Field>
</div>

<style>
  .range { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
</style>
```

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
| `class` | `string` | `""` | |

Other attributes go to the text input.
