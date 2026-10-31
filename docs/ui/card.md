# Card

A panel of related content, with a title, actions and a footer: a record's
details, a form, a summary.

<!-- example: card/basic -->
```svelte
<script>
  import { Badge, Button, Card } from "@elyra/ui";
</script>

<Card title="Ada Lovelace" description="Customer since March 2021 · Analytical Engines Ltd">
  {#snippet actions()}
    <Badge variant="success">Active</Badge>
    <Button size="sm">Edit</Button>
  {/snippet}
  <p>12 invoices, 1 860 € overdue. Prefers invoices by email.</p>
  {#snippet footer()}
    <Button size="sm" variant="ghost" href="/customers/1/invoices">All invoices</Button>
  {/snippet}
</Card>
```

`actions` is a snippet on the right of the title: a badge, a button, a
[Dropdown](dropdown.md). `footer` is a snippet along the bottom. The form
`make:resource --view` writes is a Card with a [Form](form.md) in it.

## Accessibility

- A `<section>`, named by its title (`aria-labelledby`). The title is an
  `<h2>`.

## API

### Card

| Prop | Type | Default | Description |
|---|---|---|---|
| `title` | `string` | — | The heading, which names the card. |
| `description` | `string` | — | A line under the title. |
| `class` | `string` | `""` | |
| `actions` | `Snippet` | — | On the right of the title. |
| `footer` | `Snippet` | — | Along the bottom. |
| `children` | `Snippet` | — | The body. |

Other attributes go to the `<section>`.
