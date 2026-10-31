# Switch

On or off, now. Use it for a setting that takes effect the moment it's
flipped. For a choice that's saved with a form, use a
[Checkbox](checkbox.md).

<!-- example: switch/basic -->
```svelte
<script>
  import { Switch } from "@elyra/ui";

  let autostart = $state(true);
  let saved = $state("");
</script>

<Switch bind:checked={autostart} onchange={(on) => (saved = on ? "Starts with the computer." : "Won't start on its own.")}>
  Open at login
</Switch>
<Switch disabled>Beta updates</Switch>
<span>{saved}</span>
```

`onchange(checked)` is told after each flip. Save the setting there.

## Keyboard

| Key | |
|---|---|
| <kbd>Space</kbd>, <kbd>Enter</kbd> | Flips it. |

Clicking the label does too.

## Accessibility

- A `<button role="switch">` with `aria-checked`. The label is a
  `<label for>` it.
- In a `Field`, it has the field's id and `aria-describedby`.

## API

### Switch

| Prop | Type | Default | Description |
|---|---|---|---|
| `checked` (bindable) | `boolean` | `false` | |
| `disabled` | `boolean` | `false` | |
| `onchange` | `(checked: boolean) => void` | — | Told after each flip. |
| `class` | `string` | `""` | Added to the switch's outer element. |
| `children` | `Snippet` | — | The label. |

Other attributes go to the `<button>`.
