# Badge

A short label: a status, a count, a tag.

<!-- example: badge/variants -->
```svelte
<script>
  import { Badge } from "@elyra/ui";
</script>

<Badge>Draft</Badge>
<Badge variant="accent">New</Badge>
<Badge variant="success">Paid</Badge>
<Badge variant="warning">Due soon</Badge>
<Badge variant="danger">Overdue</Badge>
```

Pick the variant by what it means, not by the colour you want: `success` for
paid, `warning` for due soon, `danger` for overdue, `accent` for what's new,
and `neutral` for the rest. In a [Table](table.md), draw it from the `cell`
snippet.

## Accessibility

- A `<span>`: what's written in it is what's read. A colour alone says
  nothing to a screen reader, so write the status, not just a dot.

## API

### Badge

| Prop | Type | Default | Description |
|---|---|---|---|
| `variant` | `"neutral" \| "accent" \| "success" \| "warning" \| "danger"` | `"neutral"` | |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The label. |

Other attributes go to the `<span>`.
