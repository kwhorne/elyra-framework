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
- `<StatusBar>` shows offline, the outbox, transfers and the assistant. Its
  outbox counts open `<OutboxDrawer>`: refused writes with Keep mine / Take
  theirs (a conflict) or Send again / Discard. Use it rather than building
  an outbox screen; pass `describe` to name the writes.
- Keyboard and ARIA are built in: menus, selects, tabs, dialogs (focus
  trapped and given back).

## Which component

| Need | Use |
|---|---|
| An action | `Button` (`variant` primary / ghost / danger, `size="sm"`, `href="/path"`) |
| More actions | `Dropdown` + `MenuItem` |
| A form | `Form`, `Field` around `Input`, `Textarea`, `Select`, `Combobox`, `Checkbox`, `Switch`, `RadioGroup` |
| Pick one of many, or a record on the server | `Combobox` (`options` a list, or `(search) => api.customers_index({ search })` with `optionValue="id"` `optionLabel="name"`, and `label` for the current record's text) |
| A list of records | `Table` (+ `Pagination` on its own) |
| A dialog, a side panel | `Modal`, `Drawer` (`bind:open`, `footer` snippet) |
| A hint, a small panel | `Tooltip`, `Popover` |
| The window | `AppShell` (toolbar, `nav`, status bar), `Sidebar`, `NavItem`, `Toolbar` |
| A list beside its record, an editor above its preview | `SplitPane` (`first` / `second` snippets, `bind:size`, `min` / `max`, `persist="key"` keeps the size in the settings store) |
| Views of one record | `Tabs`, `Card` |
| A message | `Callout` (`danger` / `warning` are alerts), `Badge`, `Empty` |
| Waiting | `Spinner`, `Skeleton` |
| Offline writes the server refused | `OutboxDrawer` (the `StatusBar` opens it) |

Strings in components: pass translated words (`labels` on `Table`, the
`label` props) from `$t` in an app with `lang/`.

## Each component's page

Every prop, snippet, key and ARIA attribute, with examples:
`https://github.com/kwhorne/elyra-framework/blob/main/docs/ui/<page>.md`.
The pages are `button` (with ButtonGroup), `dropdown` (MenuItem), `form`
(Field), `input` (Textarea), `select`, `checkbox`, `switch`, `radio-group`,
`modal` (Drawer), `popover`, `tooltip`, `table` (Pagination), `badge`,
`app-shell` (Sidebar, NavItem, Toolbar), `status-bar`, `tabs`, `card`,
`separator`, `callout`, `outbox-drawer`, `empty`, `spinner` (Skeleton) and
`tokens`.

## Changing a component

- Styling: override a token in `app.css` (`:root { --elyra-accent: … }`),
  or pass `class`.
- More than that: `rata make:ui <Component>` copies it into
  `app/src/lib/ui/`; import the copy where it should be used. The copy is
  the app's; rata never overwrites it.

## Generated screens

`rata make:resource <Model> --view` writes the list, form and detail view on
these components. Change them freely; they're the app's.
