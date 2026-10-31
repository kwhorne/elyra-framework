# Callout

A message in the page: what the user should know about what they're
looking at. It stays where it is, unlike a toast.

<!-- example: callout/variants -->
```svelte
<script>
  import { Callout } from "@elyra/ui";
</script>

<div class="stack">
  <Callout title="Syncs every 5 minutes">Changes made on the web show up here on their own.</Callout>
  <Callout variant="success" title="Paid on time">INV-2033 was paid on 2 October.</Callout>
  <Callout variant="warning" title="Due in 3 days">INV-2050, 420 €.</Callout>
  <Callout variant="danger" title="2 invoices overdue">INV-2041 and INV-2047, 1 860 € in all.</Callout>
</div>

<style>
  .stack { display: grid; gap: 10px; width: 100%; }
</style>
```

`info` is the default. Use `success` for what went well, `warning` for what
needs a look soon, and `danger` for what's wrong. A
[Table](table.md) whose query fails says so in a `danger` Callout.

## Dismissible

`dismissible` adds a close button. The callout hides itself, and
`ondismiss` is told, so the app can remember not to show it again.

<!-- example: callout/dismissible -->
```svelte
<script>
  import { Callout } from "@elyra/ui";

  let hidden = $state(false);
</script>

<Callout variant="info" title="New: the assistant" dismissible ondismiss={() => (hidden = true)}>
  Press the Assistant button in the status bar and ask about a customer.
</Callout>
{#if hidden}<p>Dismissed. Reload to see it again.</p>{/if}
```

## Accessibility

- A `warning` or a `danger` callout is a `role="alert"`, read as soon as it
  appears. An `info` or `success` one is a `role="status"`, read when the
  reader is free.
- The close button is named "Dismiss".

## API

### Callout

| Prop | Type | Default | Description |
|---|---|---|---|
| `variant` | `"info" \| "success" \| "warning" \| "danger"` | `"info"` | |
| `title` | `string` | — | |
| `dismissible` | `boolean` | `false` | A close button. |
| `ondismiss` | `() => void` | — | Told when it's closed. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The message. |
