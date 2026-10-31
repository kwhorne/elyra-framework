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
| **Offline since 14:02** | The app has lost the backend. | `connection` |
| **3 changes waiting** | Writes made offline are queued in the outbox. | `outbox` |
| **1 change refused** | Laravel turned a queued write down. | `outbox` |
| **Uploading report.pdf 40 %** | An upload or a download is running. | `transfers` |
| **Assistant** | The app has an [assistant](../assistant.md). The button opens its panel. | `assistant` |

Turn any of them off: `connection={false}`, `outbox={false}`,
`transfers={false}`, `assistant={false}`. `assistant` is `"auto"` by
default, which shows the button when the app has an assistant. `true` shows
it whether or not the app has one.

The app's own items go in as children, before the assistant's button: a
count, a sync time, the signed-in user.

## Accessibility

- A `<footer>` named "Status". Going offline is a `role="status"`, so a
  screen reader says it when it happens.
- A transfer's spinner is named for what it's doing.

## API

### StatusBar

| Prop | Type | Default | Description |
|---|---|---|---|
| `connection` | `boolean` | `true` | Offline, and since when. |
| `outbox` | `boolean` | `true` | Changes waiting, and refused. |
| `transfers` | `boolean` | `true` | Uploads and downloads. |
| `assistant` | `"auto" \| boolean` | `"auto"` | The assistant's button. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The app's own items. |
