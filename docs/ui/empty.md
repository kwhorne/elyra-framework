# Empty

Nothing here yet. Say what would be here, and give the action that starts
it.

<!-- example: empty/basic -->
```svelte
<script>
  import { Button, Empty } from "@elyra/ui";
</script>

<Empty title="No notes yet" description="Notes are visible to everyone who can see this customer.">
  {#snippet action()}
    <Button size="sm" variant="primary">Add a note</Button>
  {/snippet}
</Empty>
```

An empty [Table](table.md) shows one of its own: "Nothing here yet", or
"Nothing matches" a search. Its `empty` snippet replaces it.

## API

### Empty

| Prop | Type | Default | Description |
|---|---|---|---|
| `title` | `string` | `"Nothing here yet"` | |
| `description` | `string` | — | What would be here, and why it isn't. |
| `class` | `string` | `""` | |
| `action` | `Snippet` | — | The button that starts it. |
