# StatusBar

The bar along the bottom of the window. It says what the app is doing that
the user would want to know, and says nothing while all is well. The
[AppShell](app-shell.md) has one.

<!-- example: status-bar/basic -->
```svelte
<script>
  import { StatusBar } from "@elyra/ui";
</script>

<!-- Offline, the outbox and transfers show up when they happen. -->
<StatusBar assistant={true}>
  <span>4 customers</span>
</StatusBar>
```

It follows the runtime's stores, so there's nothing to wire:

| It shows | When | From |
|---|---|---|
| **Offline since 14:02** | The app has lost the backend. **Offline (simulated)** when the [inspector](../inspector.md#trying-the-app-offline) has it unplugged. | `connection` |
| **3 changes waiting** | Writes made offline are queued in the outbox. Click it for the [OutboxDrawer](outbox-drawer.md). | `outbox` |
| **1 change refused** | Laravel turned a queued write down. Click it for the [OutboxDrawer](outbox-drawer.md), to keep, retry or discard it. | `outbox` |
| **Uploading report.pdf 40 %** | An upload or a download is running. | `transfers` |
| **Editing contract.docx** | A file is open in the user's own app, and each save goes back ([RFC 0038](../backend.md#edit-it-in-its-own-app)). It says **Saving contract.docx** while one is sent, **2 saves waiting** while the server isn't answering, and **1 file needs you** when it changed on the server too or was turned down. Click it for the [EditingDrawer](editing-drawer.md). | `editing` |
| **Assistant** | The app has an [assistant](../assistant.md). The button opens its panel. | `assistant` |
| **Inspector** | A debug build. The button opens the [inspector](../inspector.md), and says **Simulating −2** while abilities are turned off there, **Simulating offline** while the backend is unplugged, or both. | `inspector` |

Turn any of them off: `connection={false}`, `outbox={false}`,
`transfers={false}`, `editing={false}`, `assistant={false}`,
`inspector={false}`. `assistant`
and `inspector` are `"auto"` by default, which shows the button when the
app has one. `true` shows it whether or not it does.

The app's own items go in as children, before the assistant's button: a
count, a sync time, the signed-in user.

## Accessibility

- A `<footer>` named "Status". Going offline is a `role="status"`, so a
  screen reader says it when it happens.
- A transfer's spinner is named for what it's doing.
- The outbox's counts and the editing line are buttons that open a dialog
  (`aria-haspopup`). Focus goes back to the button when it closes.

## API

### StatusBar

| Prop | Type | Default | Description |
|---|---|---|---|
| `connection` | `boolean` | `true` | Offline, and since when. |
| `outbox` | `boolean` | `true` | Changes waiting, and refused, which open the [OutboxDrawer](outbox-drawer.md). |
| `transfers` | `boolean` | `true` | Uploads and downloads. |
| `editing` | `boolean` | `true` | Files open in their own apps, which open the [EditingDrawer](editing-drawer.md). |
| `assistant` | `"auto" \| boolean` | `"auto"` | The assistant's button. |
| `inspector` | `"auto" \| boolean` | `"auto"` | The inspector's button, in a debug build. |
| `labels` | `object` | `{}` | Its words, for this instance: the app's translations otherwise ([its words](../i18n.md#the-frameworks-own-words)). |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The app's own items. |
