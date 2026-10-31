# Modal and Drawer

A dialog over the window, for what has to be done or answered before going
on. Focus moves into it and stays there until it closes, then goes back to
where it was.

<!-- example: modal/basic -->
```svelte
<script>
  import { Button, Modal } from "@elyra/ui";

  let open = $state(false);
</script>

<Button variant="danger" onclick={() => (open = true)}>Delete customer</Button>

<Modal bind:open title="Delete Ada Lovelace?" description="Her 12 invoices stay in the accounts." size="sm">
  <p>This can't be undone.</p>
  {#snippet footer()}
    <Button onclick={() => (open = false)}>Cancel</Button>
    <Button variant="danger" onclick={() => (open = false)}>Delete</Button>
  {/snippet}
</Modal>
```

`bind:open` opens and closes it. `footer` is a snippet for its buttons,
along the bottom. `size` is `sm` (360px), `md` (460px) or `lg` (680px), and
never wider than the window.

For a yes or no, you don't need a Modal: `confirm()` from `@elyra/runtime`,
or `confirm` on a [Button](button.md), asks in the runtime's own dialog.

## A form in a modal

A footer button can submit a form in the body: give the `Form` an `id`, and
the button `form` with it. `dismissible={false}` keeps the modal open until
the user picks a button. Esc, a click outside and the close button do
nothing, and the close button isn't drawn.

<!-- example: modal/form -->
```svelte
<script>
  import { Button, Field, Form, Input, Modal } from "@elyra/ui";

  let open = $state(false);
  let name = $state("");

  async function create() {
    await new Promise((resolve) => setTimeout(resolve, 500));
    open = false;
  }
</script>

<Button variant="primary" onclick={() => (open = true)}>New customer</Button>

<Modal bind:open title="New customer" dismissible={false}>
  <Form onsubmit={create} id="new-customer">
    <Field name="name" label="Name" required><Input bind:value={name} /></Field>
  </Form>
  {#snippet footer()}
    <Button onclick={() => (open = false)}>Cancel</Button>
    <Button type="submit" form="new-customer" variant="primary">Create</Button>
  {/snippet}
</Modal>
```

## Drawer

The same panel at the side of the window, full height: filters, a record's
details, settings. `side` is `right` or `left`. It's a Modal underneath, so
it takes the same `title`, `description`, `footer`, `dismissible` and
`onclose`, and behaves the same.

<!-- example: modal/drawer -->
```svelte
<script>
  import { Button, Drawer, Field, Select, Switch } from "@elyra/ui";

  let open = $state(false);
  let country = $state(undefined);
  let active = $state(true);
</script>

<Button onclick={() => (open = true)}>Filters</Button>

<Drawer bind:open title="Filters">
  <Field label="Country"><Select bind:value={country} options={["Denmark", "Norway", "Sweden"]} placeholder="Any" /></Field>
  <Switch bind:checked={active}>Only active customers</Switch>
  {#snippet footer()}
    <Button variant="primary" onclick={() => (open = false)}>Apply</Button>
  {/snippet}
</Drawer>
```

## Closing

The user closes it with <kbd>Esc</kbd>, the close button, or a click
outside, unless it's `dismissible={false}`. `onclose` is told then. Setting
`open` to `false` yourself closes it without calling `onclose`.

## Keyboard

| Key | |
|---|---|
| <kbd>Tab</kbd>, <kbd>Shift</kbd> <kbd>Tab</kbd> | Move through what's in the dialog, round from the last to the first. |
| <kbd>Esc</kbd> | Closes it, when it's dismissible. |

When it opens, focus moves to the first thing in it that takes focus. When
it closes, focus goes back to what had it before.

## Accessibility

- `role="dialog"` with `aria-modal="true"`. It's named by its `title`
  (`aria-labelledby`) and described by its `description`.
- Focus is trapped inside while it's open, and given back when it closes.
- The close button is named "Close".

## API

### Modal

| Prop | Type | Default | Description |
|---|---|---|---|
| `open` (bindable) | `boolean` | `false` | |
| `title` | `string` | — | The heading, which names the dialog. |
| `description` | `string` | — | A line under the title, which describes it. |
| `size` | `"sm" \| "md" \| "lg"` | `"md"` | Its width. |
| `dismissible` | `boolean` | `true` | Whether Esc, a click outside and the close button close it. |
| `placement` | `"center" \| "left" \| "right"` | `"center"` | Where it is. Use a `Drawer` for the sides. |
| `onclose` | `() => void` | — | Told when the user closes it. |
| `class` | `string` | `""` | Added to the dialog. |
| `children` | `Snippet` | — | The body. |
| `footer` | `Snippet` | — | The buttons, along the bottom. |

### Drawer

| Prop | Type | Default | Description |
|---|---|---|---|
| `open` (bindable) | `boolean` | `false` | |
| `side` | `"right" \| "left"` | `"right"` | |
| `children` | `Snippet` | — | The body. |
| `footer` | `Snippet` | — | The buttons, along the bottom. |

Other attributes are the Modal's: `title`, `description`, `dismissible`,
`onclose`, `class`.
