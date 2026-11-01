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
| **Assistant** | The app has an [assistant](../assistant.md). The button opens its panel. | `assistant` |
| **Inspector** | A debug build. The button opens the [inspector](../inspector.md), and says **Simulating −2** while abilities are turned off there, **Simulating offline** while the backend is unplugged, or both. | `inspector` |

Turn any of them off: `connection={false}`, `outbox={false}`,
`transfers={false}`, `assistant={false}`, `inspector={false}`. `assistant`
and `inspector` are `"auto"` by default, which shows the button when the
app has one. `true` shows it whether or not it does.

The app's own items go in as children, before the assistant's button: a
count, a sync time, the signed-in user.

## Accessibility

- A `<footer>` named "Status". Going offline is a `role="status"`, so a
  screen reader says it when it happens.
- A transfer's spinner is named for what it's doing.
- The outbox's counts are buttons that open a dialog (`aria-haspopup`).
  Focus goes back to the count when it closes.

## API

### StatusBar

| Prop | Type | Default | Description |
|---|---|---|---|
| `connection` | `boolean` | `true` | Offline, and since when. |
| `outbox` | `boolean` | `true` | Changes waiting, and refused, which open the [OutboxDrawer](outbox-drawer.md). |
| `transfers` | `boolean` | `true` | Uploads and downloads. |
| `assistant` | `"auto" \| boolean` | `"auto"` | The assistant's button. |
| `inspector` | `"auto" \| boolean` | `"auto"` | The inspector's button, in a debug build. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The app's own items. |
