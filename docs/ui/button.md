# Button

A button, or a link that looks like one. It knows the user's abilities, can
ask before it acts, and shows when the form it submits is saving.

<!-- example: button/variants -->
```svelte
<script>
  import { Button } from "@elyra/ui";
</script>

<Button variant="primary">New customer</Button>
<Button>Export</Button>
<Button variant="ghost">Cancel</Button>
<Button variant="danger">Delete</Button>
```

Use one `primary` button for a screen's main action. `danger` is for what
destroys, and `ghost` is for what's quiet: a cancel, a toolbar's tools.

## Sizes

`sm` is for toolbars, table rows and anywhere dense.

<!-- example: button/sizes -->
```svelte
<script>
  import { Button } from "@elyra/ui";
</script>

<Button variant="primary">Medium</Button>
<Button variant="primary" size="sm">Small</Button>
<Button size="sm">Small</Button>
```

## Busy and disabled

`loading` shows a spinner and sets `aria-busy`. The button can't be pressed
while it's busy. Inside a [Form](form.md), a `type="submit"` button is busy
on its own while the form saves.

<!-- example: button/loading -->
```svelte
<script>
  import { Button } from "@elyra/ui";

  let syncing = $state(false);

  async function sync() {
    syncing = true;
    await new Promise((resolve) => setTimeout(resolve, 1500));
    syncing = false;
  }
</script>

<Button variant="primary" loading={syncing} onclick={sync}>Sync now</Button>
<Button disabled>Archived</Button>
```

## Links

With `href` it's a link. An app path (`/customers/7`) goes through the app's
router. Any other URL is used as it is, and other attributes, like `target`,
go to the `<a>`. A disabled or busy link is drawn as a disabled button.

<!-- example: button/links -->
```svelte
<script>
  import { Button } from "@elyra/ui";
  import { route } from "@elyra/runtime";
</script>

<Button href="/customers/7">Open Ada Lovelace</Button>
<Button variant="ghost" href="https://elyracode.com/framework" target="_blank">The website</Button>
<span>The app is at <code>{$route.path}</code></span>
```

## What the user may do

`can` names an ability. A user without it doesn't see the button, or, with
`canMode="disable"`, sees it disabled. The abilities come from Laravel
([RFC 0010](../backend.md#what-the-user-may-do)), or from the app's own
grants when it has no backend. Hiding a button doesn't protect anything:
the command, and Laravel, still refuse.

<!-- example: button/abilities -->
```svelte
<script>
  import { Button } from "@elyra/ui";
</script>

<!-- The user may export customers, but not delete them. -->
<Button can="customers.export">Export</Button>
<Button variant="danger" can="customers.delete">Delete (hidden)</Button>
<Button variant="danger" can="customers.delete" canMode="disable">Delete</Button>
```

In these docs the user may do everything but `customers.delete`.

## Asking first

`confirm` asks in the runtime's dialog, the one `confirm()` from
`@elyra/runtime` opens, before `onclick` runs. `confirmLabel` names the
dialog's button. A `danger` button's dialog is a danger one. On a link or a
submit button, the yes follows the link or submits the form.

<!-- example: button/confirm -->
```svelte
<script>
  import { Button } from "@elyra/ui";

  let deleted = $state(false);
</script>

<Button
  variant="danger"
  confirm="Delete Ada Lovelace? Her 12 invoices stay in the accounts."
  confirmLabel="Delete customer"
  onclick={() => (deleted = true)}
>
  Delete customer
</Button>
{#if deleted}<span>Deleted.</span>{/if}
```

## ButtonGroup

Buttons that belong together, joined. `label` names the group for a screen
reader.

<!-- example: button/group -->
```svelte
<script>
  import { Button, ButtonGroup } from "@elyra/ui";

  let view = $state("list");
</script>

<ButtonGroup label="View">
  {#each ["list", "board", "calendar"] as option (option)}
    <Button size="sm" variant={view === option ? "primary" : "default"} aria-pressed={view === option} onclick={() => (view = option)}>
      {option[0].toUpperCase() + option.slice(1)}
    </Button>
  {/each}
</ButtonGroup>
```

## Keyboard

| Key | |
|---|---|
| <kbd>Enter</kbd>, <kbd>Space</kbd> | Presses the button. A link opens on <kbd>Enter</kbd>. |
| <kbd>Tab</kbd> | Moves past it. A disabled, busy or hidden button isn't reached. |

## Accessibility

- A native `<button>`, or an `<a>` with `href`: the role, focus and keys
  are the browser's.
- `loading` sets `aria-busy="true"` and disables it. The spinner is hidden
  from screen readers, so the label is what's read.
- A group is `role="group"`, named by `label`.
- A toggle in a group says whether it's on with `aria-pressed`, which goes to
  the button.

## API

### Button

| Prop | Type | Default | Description |
|---|---|---|---|
| `variant` | `"default" \| "primary" \| "ghost" \| "danger"` | `"default"` | How it looks. |
| `size` | `"md" \| "sm"` | `"md"` | `sm` for dense places. |
| `type` | `"button" \| "submit" \| "reset"` | `"button"` | A `submit` button in a `Form` is busy while it saves. |
| `href` | `string` | — | Makes it a link. An app path goes through the router. |
| `disabled` | `boolean` | `false` | |
| `loading` | `boolean` | `false` | Busy: a spinner, `aria-busy`, and it can't be pressed. |
| `can` | `string` | — | The ability it needs, such as `customers.delete`. |
| `canMode` | `"hide" \| "disable"` | `"hide"` | What a user without `can` sees. |
| `confirm` | `string` | — | A question to ask first. It goes on only on yes. |
| `confirmLabel` | `string` | — | The confirm dialog's button. |
| `onclick` | `(event) => void \| Promise<void>` | — | Runs on a press, after a yes. |
| `class` | `string` | `""` | Added to the button's classes. |
| `children` | `Snippet` | — | The label. |

Other attributes go to the `<button>` (or the `<a>`): `title`,
`aria-label`, `aria-pressed`, `form`, `target`.

### ButtonGroup

| Prop | Type | Default | Description |
|---|---|---|---|
| `label` | `string` | — | The group's name, for a screen reader. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The buttons. |

Other attributes go to the group's `<div>`.
