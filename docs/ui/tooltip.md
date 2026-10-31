# Tooltip

A short hint for what's inside: what an icon button does, or what a word
means. It shows on hover, after a moment, and at once on keyboard focus.

<!-- example: tooltip/basic -->
```svelte
<script>
  import { Button, Tooltip } from "@elyra/ui";
</script>

<Tooltip text="Copies the customer's link">
  <Button variant="ghost">Copy link</Button>
</Tooltip>
<Tooltip text="Sent to the accountant every Friday" placement="bottom">
  <Button variant="ghost">Weekly report</Button>
</Tooltip>
```

The tooltip describes the first element inside it: that's what a screen
reader reads it with. Put one element in, usually a button.

Keep it to a few words that add something. The hint isn't there on a touch
screen, and a screen reader reads it only as a description, so nothing the
user must know belongs only in a tooltip.

## Keyboard

| Key | |
|---|---|
| <kbd>Tab</kbd> | Focus on what's inside shows the tooltip at once. Leaving hides it. |
| <kbd>Esc</kbd> | Hides it. |

## Accessibility

- The hint is a `role="tooltip"`. The element inside gets its
  `aria-describedby`.
- It's text only, so there's nothing in it to reach.

## API

### Tooltip

| Prop | Type | Default | Description |
|---|---|---|---|
| `text` | `string` | — | **Required.** The hint. |
| `placement` | `"top" \| "bottom"` | `"top"` | Above or below. |
| `delay` | `number` | `400` | Milliseconds of hovering before it shows. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The element it describes. |
