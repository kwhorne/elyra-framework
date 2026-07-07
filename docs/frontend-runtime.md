# Frontend runtime — `@elyra/runtime`

The npm package the Svelte app imports. It speaks the [binary wire
format](wire-format.md) so you don't have to.

```ts
import { invoke, channel, CommandError } from "@elyra/runtime";
```

## `invoke(command, ...args)`

Call a Rust `#[command]` by name. Arguments are MessagePack-encoded; the result
is decoded into the resolved type.

```ts
const greeting = await invoke<string>("greet", "world");
const sum = await invoke<number>("add", 2, 3);
```

If the command returns an error (a `Result::Err`, or a middleware/decode
failure), the promise **rejects** with a `CommandError` carrying the command
name and message.

```ts
try {
  await invoke("checked_div", 1, 0);
} catch (e) {
  if (e instanceof CommandError) console.error(e.message);
}
```

## `channel(name)`

Subscribe to a server-pushed [event channel](events.md). The return value is a
**Svelte-readable store**, so `$channel(...)` works in a component; it's also
usable standalone.

```svelte
<script>
  import { channel } from "@elyra/runtime";
  const cursor = channel("cursor");
</script>
<pre>{JSON.stringify($cursor)}</pre>
```

```ts
const unsub = channel<number>("tick").subscribe((v) => { /* ... */ });
// later: unsub();
```

All channels are multiplexed over one long-poll connection with automatic
reconnect/backoff.

## The generated `api.*` facade

After [`rata codegen`](codegen.md) you get `bindings.ts` with a fully typed
facade — prefer it over stringly-typed `invoke`:

```ts
import { api } from "./bindings";
const todos = await api.list_todos();        // Promise<Todo[]>
const todo  = await api.add_todo("milk");    // Promise<Todo>
```

The facade delegates to `invoke` under the hood, so error handling is identical.

## Origin

Everything is same-origin under `elyra://localhost` (the app is served there,
IPC and events too), so `fetch` needs no CORS handling in production. Under
`rata dev` the page loads from Vite's `http://localhost:5173`; the shell adds
permissive CORS to the IPC endpoints for that case.

## Related

- [Commands](commands.md) · [Events](events.md) · [Codegen](codegen.md)
- [Wire format](wire-format.md)
