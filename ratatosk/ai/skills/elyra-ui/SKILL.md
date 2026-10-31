---
name: elyra-ui
description: Build Elyra Framework screens with @elyra/ui — Svelte 5 components (Button, Form, Field, Input, Select, Table, Modal, Tabs, AppShell, StatusBar, …) on the --elyra-* design tokens, that know the app's live queries, Laravel's validation errors and the user's abilities. Use when writing or changing a Svelte view in an Elyra app.
---

# Elyra screens with @elyra/ui

Build views from `@elyra/ui`; don't hand-write buttons, inputs, dialogs or
tables, and don't add colours: the components read the `--elyra-*` tokens
from `@elyra/ui/tokens.css` (imported in `main.js`), dark and light.

```svelte
<script>
  import { api, live } from "./bindings";
  import { Button, Card, Field, Form, Input, Table } from "@elyra/ui";

  let query = $state({ page: 1 });
  const customers = $derived(live.customers_index(query));
  let draft = $state({ name: "", email: "" });
</script>

<Table page={customers} bind:query columns={["name", "email"]} sortable search label="Customers">
  {#snippet actions(row)}
    <Button size="sm" variant="danger" can="customers.delete" confirm="Delete {row.name}?"
            onclick={() => api.customers_destroy(row.id)}>Delete</Button>
  {/snippet}
</Table>

<Card title="New customer">
  <Form onsubmit={() => api.customers_store(draft)}>
    <Field name="name" label="Name" required><Input bind:value={draft.name} /></Field>
    <Field name="email" label="Email"><Input type="email" bind:value={draft.email} /></Field>
    <Button type="submit" variant="primary">Save</Button>
  </Form>
</Card>
```

## What they already do — don't redo it

- `<Form onsubmit>`: a thrown validation failure (Laravel's `422`, or a
  command's `ValidationErrors`) lands under the `<Field name>` it names;
  anything else shows at the form's top; the submit button is busy while
  it runs. Let `onsubmit` throw; don't catch and re-show errors yourself.
- `<Table page={live.x(query)} bind:query>` writes search, sort and page
  into `query`, so the live query runs again; it shows loading, failure and
  empty states. `rows={[..]}` for a plain array.
- `can="ability"` on `Button`, `MenuItem`, `NavItem` and a tab hides what
  the signed-in user may not do (`canMode="disable"` shows it disabled).
- `confirm="…"` on a `Button` asks in the runtime's dialog first.
- `<StatusBar>` shows offline, the outbox, transfers and the assistant.
- Keyboard and ARIA are built in: menus, selects, tabs, dialogs (focus
  trapped and given back).

## Which component

| Need | Use |
|---|---|
| An action | `Button` (`variant` primary / ghost / danger, `size="sm"`, `href="/path"`) |
| More actions | `Dropdown` + `MenuItem` |
| A form | `Form`, `Field` around `Input`, `Textarea`, `Select`, `Checkbox`, `Switch`, `RadioGroup` |
| A list of records | `Table` (+ `Pagination` on its own) |
| A dialog, a side panel | `Modal`, `Drawer` (`bind:open`, `footer` snippet) |
| A hint, a small panel | `Tooltip`, `Popover` |
| The window | `AppShell` (toolbar, `nav`, status bar), `Sidebar`, `NavItem`, `Toolbar` |
| Views of one record | `Tabs`, `Card` |
| A message | `Callout` (`danger` / `warning` are alerts), `Badge`, `Empty` |
| Waiting | `Spinner`, `Skeleton` |

Strings in components: pass translated words (`labels` on `Table`, the
`label` props) from `$t` in an app with `lang/`.

## Changing a component

- Styling: override a token in `app.css` (`:root { --elyra-accent: … }`),
  or pass `class`.
- More than that: `rata make:ui <Component>` copies it into
  `app/src/lib/ui/`; import the copy where it should be used. The copy is
  the app's; rata never overwrites it.

## Generated screens

`rata make:resource <Model> --view` writes the list, form and detail view on
these components. Change them freely; they're the app's.
