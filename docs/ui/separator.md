# Separator

A line between groups: between a toolbar's tools, between the links in a
sidebar, between the parts of a panel.

<!-- example: separator/basic -->
```svelte
<script>
  import { Button, Separator } from "@elyra/ui";
</script>

<div class="row">
  <Button size="sm" variant="ghost">Bold</Button>
  <Button size="sm" variant="ghost">Italic</Button>
  <Separator orientation="vertical" />
  <Button size="sm" variant="ghost">Link</Button>
</div>
<Separator />
<p>Below the line.</p>

<style>
  .row { display: flex; align-items: center; gap: 4px; height: 28px; }
</style>
```

A `vertical` separator is as tall as the row it's in.

## Accessibility

- A `role="separator"` with `aria-orientation`.

## API

### Separator

| Prop | Type | Default | Description |
|---|---|---|---|
| `orientation` | `"horizontal" \| "vertical"` | `"horizontal"` | |
| `class` | `string` | `""` | |
