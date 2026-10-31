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
