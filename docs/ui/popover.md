# Popover

A panel next to a button, for more than a menu: a filter, a few fields, a
preview. It doesn't block the window the way a [Modal](modal.md) does.

<!-- example: popover/basic -->
```svelte
<script>
  import { Checkbox, Field, Popover, Select } from "@elyra/ui";

  let status = $state("Active");
  let vip = $state(false);
</script>

<Popover label="Filter">
  <Field label="Status"><Select bind:value={status} options={["Active", "Paused", "Closed"]} /></Field>
  <Checkbox bind:checked={vip}>Only VIP customers</Checkbox>
</Popover>
<span>{status}{vip ? ", VIP" : ""}</span>
```

`label` is the button's text, and the panel's name. For more than text on
the button, give a `trigger` snippet, and keep `label` so the panel still
has a name. `variant` is the button's look. `align="end"` lines the panel up
with the button's right edge. `bind:open` opens and closes it from outside.

## Keyboard

| Key | |
|---|---|
| <kbd>Enter</kbd>, <kbd>Space</kbd> | On the button: opens or closes it. |
| <kbd>Tab</kbd> | Moves through what's in the panel, and on out of it. |
| <kbd>Esc</kbd> | Closes it; focus goes back to the button. |

When it opens, focus moves to the first thing in it. A click outside closes
it.

## Accessibility

- The button has `aria-haspopup="dialog"`, `aria-expanded` and
  `aria-controls`.
- The panel is a non-modal `role="dialog"`, named by `label`. Focus isn't
  trapped in it.

## API

### Popover

| Prop | Type | Default | Description |
|---|---|---|---|
| `open` (bindable) | `boolean` | `false` | |
| `label` | `string` | — | The button's text, and the panel's name. |
| `trigger` | `Snippet` | — | The button's content, instead of `label`. |
| `variant` | `"default" \| "primary" \| "ghost" \| "danger"` | `"default"` | The button's look. |
| `align` | `"start" \| "end"` | `"start"` | Which edge of the button the panel lines up with. |
| `class` | `string` | `""` | Added to the popover's outer element. |
| `children` | `Snippet` | — | What's in the panel. |
