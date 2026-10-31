# UI components (`@elyra/ui`)

Desktop components for Svelte 5 ([RFC 0012](proposals/0012-elyra-ui.md)),
themed by Elyra's design tokens, driven from the keyboard, and aware of the
app: the user's abilities, Laravel's validation errors, the runtime's dialogs.
`rata new` adds the package; an older app adds `@elyra/ui` to
`app/package.json` (the release's `elyra-ui-<version>.tgz`).

```svelte
<script>
  import { api } from "./bindings";
  import { Button, Field, Form, Input, Select } from "@elyra/ui";

  let customer = $state({ name: "", email: "", country: "Norway" });
</script>

<Form onsubmit={() => api.customers_store(customer)}>
  <Field name="name" label="Name" required><Input bind:value={customer.name} /></Field>
  <Field name="email" label="Email"><Input type="email" bind:value={customer.email} /></Field>
  <Field name="country" label="Country">
    <Select bind:value={customer.country} options={["Denmark", "Norway", "Sweden"]} />
  </Field>
  <Button type="submit" variant="primary">Save</Button>
</Form>
```

A `422` from Laravel, or `ValidationErrors` from a command, puts each message
under its field; the button shows the form is busy while it saves.

## Design tokens

`@elyra/ui/tokens.css`, imported in `main.js` before `app.css`, defines the
`--elyra-*` CSS variables every component, the app's own styles and the
runtime's dialogs read: surfaces (`--elyra-bg`, `--elyra-surface`,
`--elyra-raised`, `--elyra-inset`), `--elyra-text`, `--elyra-muted`,
`--elyra-border`, `--elyra-focus`, `--elyra-accent`, `--elyra-accent-soft`,
`--elyra-on-accent`, `--elyra-danger` / `-success` / `-warning`,
`--elyra-shadow`, `--elyra-overlay`, `--elyra-radius` / `-radius-lg`,
`--elyra-space` and the fonts. Dark by default, light under
`[data-theme="light"]`; override any of them in `app.css`.

## Components

Every component takes `class`, and passes other attributes to its element.

### Button

`variant` (`default`, `primary`, `ghost`, `danger`), `size` (`md`, `sm`),
`type`, `disabled`, `loading`, `onclick`. With `href` it is a link; an app
path (`/customers/7`) goes through the router.

- `can="customers.delete"` hides it from a user without the ability
  ([RFC 0010](backend.md#what-the-user-may-do)); `canMode="disable"` shows it
  disabled instead. The server still decides.
- `confirm="Delete Ada?"` asks in the runtime's dialog before it runs
  (`confirmLabel` names the dialog's button; a `danger` button's is red).
- Inside a `<Form>`, a `type="submit"` button is busy while the form saves.

`ButtonGroup` joins buttons (`label` names the group).

### Dropdown and MenuItem

```svelte
<Dropdown label="Actions" align="end">
  <MenuItem onclick={edit}>Edit</MenuItem>
  <MenuItem can="customers.delete" danger onclick={remove}>Delete</MenuItem>
</Dropdown>
```

A menu button (`role="menu"`): ↓, ↑, Enter or Space opens it, ↓/↑/Home/End
move, Enter picks, Esc closes and gives focus back. `trigger` is a snippet
for the button's content. A `MenuItem` takes `onclick`, `disabled`,
`danger`, `can` and `canMode`.

### Form and Field

`<Form onsubmit>` runs the save and catches what it throws: a validation
failure lands on the `<Field name="…">` it names, and messages for fields
the form doesn't show, or any other error, appear at its top
(`role="alert"`). `bind:errors` reads or sets the messages yourself.

`<Field>` takes `label`, `description`, `required`, `name` and `error` (a
message of its own). The control inside gets the field's id, `aria-invalid`
and `aria-describedby`.

### Input, Textarea

`bind:value`, and any input attribute (`type`, `placeholder`, `rows`).

### Select

`bind:value`, `options` (strings, or `{ value, label, disabled }`),
`placeholder`, `onchange`, `disabled`. A combobox over a listbox: ↓/↑ or
Enter opens it, ↓/↑/Home/End move past disabled options, Enter picks, Esc
closes. From 8 options it has a search box (`searchable` sets it either
way); without one, typing jumps to the option it starts.

### Checkbox, Switch, RadioGroup

`<Checkbox bind:checked>Label</Checkbox>` and
`<Switch bind:checked>Label</Switch>` (`role="switch"`, `onchange`).
`<RadioGroup bind:value options={[..]} inline>`, native radios (the arrow
keys move between them), labelled by its `<Field>`.

### Modal and Drawer

```svelte
<Modal bind:open title="Delete customer" description="It can't be undone.">
  …
  {#snippet footer()}<Button onclick={() => (open = false)}>Cancel</Button>{/snippet}
</Modal>
```

`role="dialog"`, `aria-modal`. Focus moves in and stays in (Tab and
Shift+Tab wrap), and goes back where it was when it closes. Esc, the close
button or a click outside closes it, unless `dismissible={false}`; `onclose`
is told. `size` is `sm`, `md` or `lg`. `<Drawer side="right">` is the same
panel at a side of the window.

### Tooltip and Popover

`<Tooltip text="Copies the link">…</Tooltip>` describes the element inside
(`role="tooltip"`), on hover or keyboard focus; Esc hides it.
`<Popover label="Filter">…</Popover>` is a non-modal panel next to its
button: focus moves in, Esc or a click outside closes it.

### Table and Pagination

```svelte
<script>
  import { live } from "./bindings";
  import { Button, Table } from "@elyra/ui";

  let query = $state({ page: 1 });
  const customers = $derived(live.customers_index(query));
</script>

<Table page={customers} bind:query columns={["name", "email"]} sortable search label="Customers"
       onrowclick={(row) => navigate(`/customers/${row.id}`)}>
  {#snippet actions(row)}
    <Button size="sm" variant="danger" can="customers.delete" confirm="Delete {row.name}?"
            onclick={() => api.customers_destroy(row.id)}>Delete</Button>
  {/snippet}
</Table>
```

`page` is a `live.*` store of a `Page<T>` (the shape `make:resource`'s index
command answers), a `Live`, or a plain `Page`. Search, sort and page are
written into `query` (`search`, `sort`, `direction`, `page`), so the live
query runs again; the command sorts by the columns it allows. `rows` (a plain
array) is searched, sorted and paged in the table instead (`perPage`, 25 by
default).

- `columns`: keys, or `{ key, label, sortable, align: "end", format(value, row) }`.
- `sortable`: `true` for every column, or a list of keys. A sorted header
  says so (`aria-sort`).
- `search`: a search box, sent after a short pause.
- `cell(row, column)` draws a cell, `actions(row)` the last column,
  `empty()` an empty list; `key` is the rows' id field (`id`).
- `onrowclick(row)` makes the rows a list: click, or ↓/↑ and Enter.
- Loading shows placeholder rows, a failed query a `Callout`, an empty list
  an `Empty`.

`<Pagination bind:page lastPage onchange>` is the pager on its own: Previous,
Next, and the pages around the current one (`aria-current="page"`).

### AppShell, Sidebar, NavItem, Toolbar, StatusBar

```svelte
<script>
  import { nav } from "./resources/index.js";
  import { AppShell, Button } from "@elyra/ui";
</script>

<AppShell title="CRM" {nav}>
  {#snippet actions()}<Button size="sm" variant="primary" href="/customers/new">New customer</Button>{/snippet}
  <Router {routes} />
</AppShell>
```

The window: a `Toolbar` with the app's name and `actions`, the navigation
(down the side, or `layout="topbar"`), the page in `<main>`, and the
`StatusBar`. `nav` is the resources' registry, `[{ label, path, count, can }]`;
or put `<NavItem>`s in a `navigation` snippet.

- `<NavItem path="/customers" count={unread} can="customers.view">` links
  into the app, is current (`aria-current="page"`) on that page and the ones
  under it, shows `count` (a number or a store), and hides from a user
  without the ability.
- `<Sidebar>` and `<Toolbar title>` are the parts on their own.
- `<StatusBar>` shows what the app is doing: offline since when, the
  changes waiting in the outbox (and refused), the uploads and downloads
  running, and a button for the app's assistant when it has one. Turn each
  off with `connection`, `outbox`, `transfers` or `assistant={false}`; the
  app's own items go in as children. Nothing shows while all is well.

### Tabs

```svelte
<Tabs bind:value={tab} label="Customer"
      tabs={[{ id: "details", label: "Details" }, { id: "audit", label: "Audit", can: "customers.audit" }]}>
  {#snippet panel(id)}…{/snippet}
</Tabs>
```

`role="tablist"`: ←/→, Home and End move between tabs and show them.
`can` hides a tab, `disabled` skips it.

### Card, Callout, Badge, Empty, Spinner, Skeleton, Separator

- `<Card title description>`: a panel, labelled by its title, with
  `actions` and `footer` snippets.
- `<Callout variant title dismissible>`: `info`, `success`, `warning` or
  `danger`. A warning or danger is announced (`role="alert"`).
- `<Badge variant>`: `neutral`, `accent`, `success`, `warning`, `danger`.
- `<Empty title description>` with an `action` snippet.
- `<Spinner size label>` (`role="status"`), `<Skeleton lines width height>`,
  `<Separator orientation>`.

## Owning a component

`rata make:ui <Component>` (RFC 0012, step 4) will copy a component into the
app to change. Until then, wrap it, or copy it from `ui/src/` by hand.

## Gallery

`npm run gallery` in `ui/` serves every component in both themes, working,
for a look before a release.

## Related

- [UI components in the runtime](components.md): dialogs, toasts, the ⌘K
  palette, the assistant panel.
- [Getting started](getting-started.md#default-theme): the default theme.
