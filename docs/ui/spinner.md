# Spinner and Skeleton

Something is on its way. A Spinner says the app is working. A Skeleton holds
the place of what's loading, in its shape, so the page doesn't jump when it
comes.

<!-- example: spinner/sizes -->
```svelte
<script>
  import { Spinner } from "@elyra/ui";
</script>

<Spinner size="sm" label="Syncing" />
<Spinner />
<Spinner size="lg" label="Loading the invoices" />
```

`label` is what a screen reader hears: say what's loading. A busy
[Button](button.md) has a spinner of its own (`loading`).

## Skeleton

`lines` of text, the last one shorter, or one block of a `width` and
`height`. A loading [Table](table.md) shows skeleton rows.

<!-- example: spinner/skeleton -->
```svelte
<script>
  import { Skeleton } from "@elyra/ui";
</script>

<div class="profile">
  <Skeleton width="44px" height="44px" />
  <Skeleton lines={3} />
</div>

<style>
  .profile { display: grid; grid-template-columns: auto 1fr; gap: 14px; width: 100%; max-width: 420px; }
</style>
```

## Accessibility

- A Spinner is a `role="status"` named by `label`.
- A Skeleton is hidden from screen readers. Say what's loading somewhere,
  with a Spinner or `aria-busy` on what's filling.
- With reduced motion, the spinner turns slower, and the skeleton's shimmer
  stops.

## API

### Spinner

| Prop | Type | Default | Description |
|---|---|---|---|
| `size` | `"sm" \| "md" \| "lg"` | `"md"` | 12, 16 or 24 pixels. |
| `label` | `string` | `"Loading"` | What a screen reader hears. |
| `class` | `string` | `""` | |

### Skeleton

| Prop | Type | Default | Description |
|---|---|---|---|
| `lines` | `number` | `1` | Lines of text. |
| `width` | `string` | `"100%"` | Each line's width. The last of several is 60%. |
| `height` | `string` | `"12px"` | Each line's height. |
| `class` | `string` | `""` | |
