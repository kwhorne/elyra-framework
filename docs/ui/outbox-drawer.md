# OutboxDrawer

The [outbox](../backend.md#writes-the-outbox), in a drawer. A write saved
offline waits there until the backend is back. When Laravel then turns one
down, the user has to decide what happens to it. The drawer lists what the
server refused, with what it said, and a way out of each. Below that it
lists what's still waiting.

The [StatusBar](status-bar.md) opens it when its "3 changes waiting" or
"1 change refused" is clicked, so an app with a StatusBar has it already.
Put one in yourself to open it from somewhere else: a menu, a banner, a
shortcut.

<!-- example: outbox-drawer/basic -->
```svelte
<script>
  import { Button, OutboxDrawer } from "@elyra/ui";

  let open = $state(false);
</script>

<Button onclick={() => (open = true)}>Show changes</Button>
<OutboxDrawer bind:open />
```

The writes here are made up. Each button takes one off the list until you
reload the page. In an app they come from `$outbox`.

## The ways out

| The server said | Shown as | The user can |
|---|---|---|
| `409` or `412`: the record changed meanwhile | **Changed meanwhile** | **Keep mine** sends it again without its precondition, overwriting what changed. **Take theirs** drops it, and what read the record runs again, showing the server's version. |
| `422`: not valid | **Not valid**, with each field's message beside the field | **Send again** or **Discard**. |
| `403`: not allowed | **Not allowed** | **Send again** or **Discard**. |
| `404`: not there any more | **Gone** | **Send again** or **Discard**. |
| Anything else it refused | **Refused** | **Send again** or **Discard**. |

A write that's still waiting can be discarded too. Discarding asks first,
in the runtime's dialog, since what was typed is lost. Keep mine and Take
theirs don't ask: the button already says what happens.

A refused write shows its fields as they were sent, and a file by its
name. A field the server complained about is marked, with its message
under it. A message for a field the write didn't have is listed after the
fields. Laravel's summary of a `422` ("The email has already been taken.
(and 1 more error)") isn't repeated above them.

If a way out fails (the backend is gone again, or the write was dealt
with in another window), the drawer says so under that write and leaves
the write where it is.

Send again sends the same body. A write that wasn't valid usually needs
fixing first: open the record in its form, and save it from there. To send
a fixed body from your own screen, call `outbox.retry(id, body)` from
`@elyra/runtime` (see [the outbox](../backend.md#in-the-frontend)).

## Naming the writes

By default a write is named by its method and path: "Update
/api/customers/42". `describe` names it the way your users would.

<!-- example: outbox-drawer/describe -->
```svelte
<script>
  import { Button, OutboxDrawer } from "@elyra/ui";

  let open = $state(false);

  // "PUT /api/customers/42" as the user would say it.
  const VERBS = { POST: "New", PUT: "Changed", PATCH: "Changed", DELETE: "Deleted" };
  function describe(write) {
    const [, , kind, id] = write.path.split("/");
    const thing = kind.replace(/s$/, "");
    return `${VERBS[write.method]} ${thing}${id ? ` ${id}` : ""}`;
  }
</script>

<Button onclick={() => (open = true)}>Show changes</Button>
<OutboxDrawer bind:open {describe} title="Not sent yet" />
```

`describe` gets the whole write: `{ id, method, path, body, status,
attempts, error, errors, reason, createdAt }`.

## Accessibility

- It's a [Drawer](modal.md): a dialog that keeps focus inside and hands it
  back when it closes. Esc closes it.
- "Refused" and "Waiting" are regions named by their headings, and each
  write is a list item named by its description.
- A way out that fails is a `role="alert"`.

## API

### OutboxDrawer

| Prop | Type | Default | Description |
|---|---|---|---|
| `open` (bindable) | `boolean` | `false` | |
| `side` | `"right" \| "left"` | `"right"` | Where it slides in from. |
| `title` | `string` | `"Changes"` | |
| `describe` | `(write: OutboxItem) => string` | — | Names a write. Without it: the method as a verb, and the path. |
| `class` | `string` | `""` | |
