# EditingDrawer

The files open in the user's own apps, in a drawer. A command that calls
[`Backend::edit(..).open()`](../backend.md#edit-it-in-its-own-app)
opens a file from a record in Word, Excel or Preview, and each save goes
back to the server ([RFC 0038](../proposals/0038-edit-externally.md)). The
drawer lists those files and where each save is. When one changed on the
server too, or was turned down, it says what to do.

The [StatusBar](status-bar.md) opens it from its **Editing contract.docx**
line, so an app with a StatusBar has it already. Put one in yourself to
open it from somewhere else.

<!-- example: editing-drawer/basic -->
```svelte
<script>
  import { Button, EditingDrawer } from "@elyra/ui";

  let open = $state(false);
</script>

<Button onclick={() => (open = true)}>Show files</Button>
<EditingDrawer bind:open />
```

The files here are made up. In an app they come from `$editing`.

## What it shows

Files that need the user come first, under **Needs you**; the rest under
**Open**. Each shows its name, its state, what the server said, and when
its last save reached the server.

| State | Shown as | The user can |
|---|---|---|
| `open`, `saved` | **Open**, **Saved** | **Open** it in its app again, show it in its folder (**Show in Finder** on a Mac), or say they're **Done**. |
| `saving`, `waiting` | **Saving**, **Waiting** | The same. A waiting save is sent again on its own. |
| `conflict`: it changed on the server too | **Changed on the server** | **Keep mine** sends the save over the server's version. **Take theirs** puts the server's version beside the user's file, never over it, and opens it. |
| `denied`, `failed` | **Not allowed**, **Refused** | Open it, show it, or say they're done. |

**Done** stops editing: the copy is removed. When its last save hasn't
reached the server, the drawer asks first, in the runtime's dialog, since
that save would be lost; only a yes gives it up.

If an action fails, the drawer says so under that file.

## Accessibility

- A [Drawer](modal.md), named by its `title`. Each file is a list item
  named by its file's name, in a section named by its heading.
- A failed action is a `role="alert"` under its file.

## API

### EditingDrawer

| Prop | Type | Default | Description |
|---|---|---|---|
| `open` (bindable) | `boolean` | `false` | |
| `side` | `"right" \| "left"` | `"right"` | Where it slides in from. |
| `title` | `string` | `"Editing"` | |
| `class` | `string` | `""` | |
