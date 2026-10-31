# Checkbox

A checkbox with its label beside it. It's the native input, drawn to match
the rest.

<!-- example: checkbox/basic -->
```svelte
<script>
  import { Checkbox } from "@elyra/ui";

  let vip = $state(true);
  let newsletter = $state(false);
</script>

<Checkbox bind:checked={vip}>VIP customer</Checkbox>
<Checkbox bind:checked={newsletter}>Gets the newsletter</Checkbox>
<Checkbox disabled>Archived</Checkbox>
```

A checkbox is for a choice that's saved with the rest of a form. For a
setting that takes effect at once, use a [Switch](switch.md).

## In a field

In a [Field](form.md), it takes the field's name and its state. A message
for that name goes under it.

<!-- example: checkbox/field -->
```svelte
<script>
  import { Checkbox, Field } from "@elyra/ui";
</script>

<Field name="terms" error="The terms must be accepted.">
  <Checkbox>I accept the terms</Checkbox>
</Field>
```

## Keyboard

| Key | |
|---|---|
| <kbd>Space</kbd> | Checks it, or unchecks it. |

Clicking the label does too.

## Accessibility

- A native `<input type="checkbox">` inside its `<label>`. The box you see
  is drawn over it and hidden from screen readers.
- In a `Field`, it has the field's id, `aria-invalid` and
  `aria-describedby`.

## API

### Checkbox

| Prop | Type | Default | Description |
|---|---|---|---|
| `checked` (bindable) | `boolean` | `false` | |
| `disabled` | `boolean` | `false` | |
| `class` | `string` | `""` | Added to the `<label>`. |
| `children` | `Snippet` | — | The label. |

Other attributes go to the `<input>`: `name`, `value`, `onchange`.
