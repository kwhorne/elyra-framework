# UI components (`@elyra/ui`)

Desktop components for Svelte 5 that know they're in an Elyra app. They
check what the user may do, show Laravel's validation errors under the
fields they belong to, ask through the runtime's dialog before what can't be
undone, and run a `live.*` list. They're themed by the design tokens, work
from the keyboard and carry ARIA. ([RFC 0012](proposals/0012-elyra-ui.md))

<!-- example: overview/first-screen -->
```svelte
<script>
  import { ValidationError } from "@elyra/runtime";
  import { Button, Field, Form, Input, Select } from "@elyra/ui";

  let customer = $state({ name: "Ada Lovelace", email: "ada@", country: "Norway" });
  let saved = $state(false);

  // In an app this is `api.customers_store(customer)`: Laravel's 422 (or a
  // command's ValidationErrors) is thrown the same way.
  async function save() {
    saved = false;
    await new Promise((resolve) => setTimeout(resolve, 600));
    if (!customer.email.includes(".")) {
      throw new ValidationError("customers_store", { email: ["The email must be a valid email address."] }, "{}");
    }
    saved = true;
  }
</script>

<Form onsubmit={save}>
  <Field name="name" label="Name" required>
    <Input bind:value={customer.name} />
  </Field>
  <Field name="email" label="Email" required description="Invoices go here.">
    <Input type="email" bind:value={customer.email} />
  </Field>
  <Field name="country" label="Country">
    <Select bind:value={customer.country} options={["Denmark", "Finland", "Norway", "Sweden"]} />
  </Field>
  <Button type="submit" variant="primary">{saved ? "Saved" : "Save customer"}</Button>
</Form>
```

Save it as it is: the email is wrong on purpose, and the message lands under
its field. That's Laravel's `422`, or a command's `ValidationErrors`, thrown
by the save and caught by the `Form`.

## Install

`rata new` adds the package. The app depends on the release's
`elyra-ui-<version>.tgz`, or on the checkout's `ui/` with `--elyra`. It
imports the tokens in `main.js`, and its Vite config keeps one copy of
`svelte` and `@elyra/runtime`. An app made before 0.21 adds the same three:

```json
"@elyra/ui": "https://github.com/kwhorne/elyra-framework/releases/download/v0.22.0/elyra-ui-0.22.0.tgz"
```

```js
// app/src/main.js: the tokens first, then the app's own CSS
import "@elyra/ui/tokens.css";
import "./app.css";
```

```js
// app/vite.config.js
resolve: { dedupe: ["svelte", "@elyra/runtime"] },
```

The package is Svelte source with no build step of its own. Its peers are
`svelte` 5 and `@elyra/runtime` 0.20 or later.

## What they know about the app

| | |
|---|---|
| **The user's abilities** | `can="customers.delete"` hides a [Button](ui/button.md), a [MenuItem](ui/dropdown.md), a [NavItem](ui/app-shell.md) or a [tab](ui/tabs.md) from a user without the ability. On a button, `canMode="disable"` shows it disabled instead. The abilities come from Laravel ([RFC 0010](backend.md#what-the-user-may-do)), or from the app's own grants without a backend. The server still decides. |
| **Validation** | A [Form](ui/form.md) catches what its `onsubmit` throws. Laravel's `422`, or a command's `ValidationErrors`, puts each message under its `Field`, and the rest at the top. |
| **Confirming** | `confirm="Delete Ada?"` on a [Button](ui/button.md) asks in the runtime's dialog, the same one `confirm()` opens. |
| **Live lists** | A [Table](ui/table.md) runs a `live.*` page. Its search, sort and page go into the query, so the command runs again and the list stays current. |
| **The router** | `href="/customers/7"` on a Button, and a NavItem's `path`, go through the app's router. A NavItem knows when its page is open. |
| **What the app is doing** | The [StatusBar](ui/status-bar.md) shows offline since when, the outbox, uploads and downloads, and the app's assistant. Its outbox counts open the [OutboxDrawer](ui/outbox-drawer.md), where the user keeps, retries or discards what Laravel refused. |

## The components

| | |
|---|---|
| **Actions** | [Button and ButtonGroup](ui/button.md), [Dropdown and MenuItem](ui/dropdown.md) |
| **Forms** | [Form and Field](ui/form.md), [Input and Textarea](ui/input.md), [Select](ui/select.md), [Checkbox](ui/checkbox.md), [Switch](ui/switch.md), [RadioGroup](ui/radio-group.md) |
| **Overlays** | [Modal and Drawer](ui/modal.md), [Popover](ui/popover.md), [Tooltip](ui/tooltip.md) |
| **Data** | [Table and Pagination](ui/table.md), [Badge](ui/badge.md) |
| **Layout** | [AppShell, Sidebar, NavItem and Toolbar](ui/app-shell.md), [StatusBar](ui/status-bar.md), [Tabs](ui/tabs.md), [Card](ui/card.md), [Separator](ui/separator.md) |
| **Feedback** | [Callout](ui/callout.md), [OutboxDrawer](ui/outbox-drawer.md), [Empty](ui/empty.md), [Spinner and Skeleton](ui/spinner.md) |

## How they're used

- **Import from the package:** `import { Button, Table } from "@elyra/ui"`.
- **Values are bound:** `bind:value` on the inputs, `bind:checked` on a
  Checkbox or Switch, `bind:open` on the overlays, `bind:query` on a Table,
  `bind:page` on Pagination.
- **Snippets fill the parts:** a Card's `actions` and `footer`, a Table's
  `cell(row, column)` and `actions(row)`, a Tab's `panel(id)`.
- **Every component takes `class`**, added to its outer element. Most pass
  any other attribute to the element they're built on. Each page's API says
  which element that is.
- **Your own styles use the [tokens](ui/tokens.md)**, so they follow the
  theme with everything else. The components add no colours of their own.
- **Words:** Table and Pagination take `labels` for theirs. The few others
  (Select's "Search…", the StatusBar's, the close buttons' names) are English
  for now. To change them, copy the component into the app (below).

## Generated screens

`rata make:resource Customer --view` writes the list and the form on these
components: a `Table` over the live index command, and a `Card` with a
`Form` and a `Field` per field. The delete is behind a `confirm`, and the
buttons are behind the user's abilities. They're the app's files from then
on.

## Owning a component

When a component needs more than its props, copy it into the app:

```
$ rata make:ui Table
wrote app/src/lib/ui/Table.svelte
```

The copy comes from the `@elyra/ui` the app has installed, so it matches the
rest of the package. It starts with a line saying which version. Its imports
of the other components and of the package's internals point back at
`@elyra/ui`, so a copied `Field` still gets its errors from `@elyra/ui`'s
`Form`. Import your copy where you want it instead of the package's; the
other views keep the package's.

The file is the app's. rata won't overwrite it (`--force` replaces it), and
upgrading `@elyra/ui` doesn't touch it. `rata make:ui --list` shows what can
be copied.

## This reference

These pages are `docs/ui/` in the repository. `npm run docs` in `ui/` serves
them with every example running, in both themes. The examples are
`ui/docs/examples/`, and the tests compile and mount each one. They also hold
every page's props table to the component's own props.

## Related

- [UI components in the runtime](components.md): dialogs, toasts, the ⌘K
  palette, the assistant panel.
- [Coding agents](ai-agents.md): `rata ai:install` teaches them these
  components.
- [Getting started](getting-started.md#default-theme): the default theme.
