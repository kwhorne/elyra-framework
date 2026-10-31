# Dropdown

A button that opens a menu of choices: a row's actions, the user's menu, an
export format. The menu is a real `role="menu"`, so it's used from the
keyboard the way a desktop menu is.

<!-- example: dropdown/basic -->
```svelte
<script>
  import { Dropdown, MenuItem } from "@elyra/ui";

  let last = $state("nothing yet");
</script>

<Dropdown label="Actions">
  <MenuItem onclick={() => (last = "Edit")}>Edit</MenuItem>
  <MenuItem onclick={() => (last = "Duplicate")}>Duplicate</MenuItem>
  <MenuItem disabled>Archive</MenuItem>
  <MenuItem danger can="customers.delete" canMode="disable">Delete</MenuItem>
</Dropdown>
<span>Picked: {last}</span>
```

Picking an item runs its `onclick` and closes the menu. A disabled item is
skipped by the arrow keys. `danger` colours what destroys, and `can` hides an
item from a user without the ability, or shows it disabled with
`canMode="disable"`.

## The trigger

`label` is the button's text. For more than text, give a `trigger` snippet.
`variant` and `size` are a [Button](button.md)'s. `align="end"` lines the
menu up with the button's right edge, for a menu at the end of a row or a
toolbar.

<!-- example: dropdown/trigger -->
```svelte
<script>
  import { Badge, Dropdown, MenuItem } from "@elyra/ui";
</script>

<Dropdown variant="ghost" align="end">
  {#snippet trigger()}
    <Badge variant="accent">AL</Badge> Ada Lovelace
  {/snippet}
  <MenuItem>Profile</MenuItem>
  <MenuItem>Keyboard shortcuts</MenuItem>
  <MenuItem danger>Sign out</MenuItem>
</Dropdown>
```

## In a table row

A row's actions fit in a small ghost dropdown, at the end:

```svelte
<Table rows={customers} columns={["name", "email"]}>
  {#snippet actions(row)}
    <Dropdown label="More" size="sm" variant="ghost" align="end">
      <MenuItem onclick={() => navigate(`/customers/${row.id}/edit`)}>Edit</MenuItem>
      <MenuItem danger can="customers.delete" onclick={() => remove(row)}>Delete</MenuItem>
    </Dropdown>
  {/snippet}
</Table>
```

A `MenuItem` has no `confirm`. For a delete, ask in its `onclick` with
`confirm()` from `@elyra/runtime`.

## Shortcuts

`shortcut` draws an item's key at its end, as the platform writes it, and
gives the item `aria-keyshortcuts`. It only shows the key; the app
registers it. See [Kbd](kbd.md#in-a-menu).

## Keyboard

| Key | |
|---|---|
| <kbd>↓</kbd>, <kbd>Enter</kbd>, <kbd>Space</kbd> | On the button: opens the menu, on the first item. |
| <kbd>↑</kbd> | On the button: opens the menu, on the last item. |
| <kbd>↓</kbd> <kbd>↑</kbd> | Moves through the items, past the disabled ones, round from the end. |
| <kbd>Home</kbd> <kbd>End</kbd> | The first item, the last item. |
| <kbd>Enter</kbd>, <kbd>Space</kbd> | Picks the item. |
| <kbd>Esc</kbd> | Closes the menu, and focus goes back to the button. |
| <kbd>Tab</kbd> | Closes the menu and moves on. |

A click outside closes it too.

## Accessibility

- The button has `aria-haspopup="menu"`, `aria-expanded` and
  `aria-controls`.
- The menu is `role="menu"`, labelled by the button. Each item is a
  `role="menuitem"` button, reached with the arrow keys (`tabindex="-1"`),
  not with Tab.

## API

### Dropdown

| Prop | Type | Default | Description |
|---|---|---|---|
| `label` | `string` | — | The button's text. |
| `trigger` | `Snippet` | — | The button's content, instead of `label`. |
| `variant` | `"default" \| "primary" \| "ghost" \| "danger"` | `"default"` | The button's look. |
| `size` | `"md" \| "sm"` | `"md"` | The button's size. |
| `align` | `"start" \| "end"` | `"start"` | Which edge of the button the menu lines up with. |
| `disabled` | `boolean` | `false` | The menu won't open. |
| `class` | `string` | `""` | Added to the dropdown's outer element. |
| `children` | `Snippet` | — | The `MenuItem`s. |

### MenuItem

| Prop | Type | Default | Description |
|---|---|---|---|
| `onclick` | `(event) => void \| Promise<void>` | — | Runs when it's picked, after the menu closes. |
| `disabled` | `boolean` | `false` | |
| `danger` | `boolean` | `false` | Coloured as a danger. |
| `can` | `string` | — | The ability it needs. |
| `canMode` | `"hide" \| "disable"` | `"hide"` | What a user without `can` sees. |
| `shortcut` | `string` | — | Its key, drawn at its end with a [`Kbd`](kbd.md) (`"CmdOrCtrl+E"`). The app registers the key. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The label. |

Other attributes go to the item's `<button>`.
