# Breadcrumbs

Where this page is, from the top: Customers › Acme AS › Invoices. Each
step is an app path (`path`, through the router, as a
[NavItem](app-shell.md) is) or a link anywhere else (`href`); the last is
this page, and isn't a link.

<!-- example: breadcrumbs/trail -->
```svelte
<script>
  import { Breadcrumbs } from "@elyra/ui";

  const trail = [
    { label: "Customers", path: "/customers" },
    { label: "Acme AS", path: "/customers/7" },
    { label: "Invoices", path: "/customers/7/invoices" },
    { label: "2026-114", path: "/customers/7/invoices/114" },
    { label: "Lines" },
  ];
</script>

<div class="stack">
  <Breadcrumbs items={trail.slice(0, 3)} />
  <Breadcrumbs items={trail} max={3} />
</div>

<style>
  .stack { display: grid; gap: 12px; }
</style>
```

A trail longer than `max` keeps the first step and the last `max - 1`, and
folds the middle into "…", whose title says what's folded. It's a `<nav>`
named "Breadcrumb", with the current page marked (`aria-current="page"`).

## API

### Breadcrumbs

| Prop | Type | Default | Description |
|---|---|---|---|
| `items` | `{ label, path?, href? }[]` | `[]` | The steps, from the top. The last is this page. |
| `max` | `number` | `4` | How many are shown before the middle folds. |
| `label` | `string` | `"Breadcrumb"` | The `<nav>`'s name. |
| `class` | `string` | `""` | |
