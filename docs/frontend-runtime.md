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

## Origin, CORS, and the IPC token

Everything is same-origin under `elyra://localhost` (the app is served there,
IPC and events too), so `fetch` needs no CORS handling in production — and the
shell sends **no** `Access-Control-Allow-*` headers there, so a foreign origin
can't read an IPC response. Under `rata dev` the page loads from Vite's
`http://localhost:5173`; only then does the shell add CORS, and only for that
exact origin (from `ELYRA_DEV_URL`).

On top of the origin rule, every `/__*` request must carry this run's **IPC
token**. The shell generates a random token per launch and injects it into the
webview before any page script runs (`globalThis.__ELYRA__.token`);
`@elyra/runtime` attaches it as `x-elyra-token` automatically. Requests without
it get `403` and the runtime throws `ForbiddenError`:

```ts
import { invoke, ForbiddenError } from "@elyra/runtime";

try {
  await invoke("greet", "World");
} catch (e) {
  if (e instanceof ForbiddenError) {
    // Refused at the security gate. `e.detail` is the shell's own explanation:
    // a bad token, an ungranted capability, or a command's `can = "…"` ability.
    console.error(e.detail);
  }
}
```

`ForbiddenError` covers every refusal at the gate, not only the token — an
ungranted [capability](security.md#3-capabilities) and an ungranted
[command ability](security.md#4-per-command-abilities) arrive the same way.
`e.detail` names which one, so read it before assuming the token is at fault.

If you talk to the bridge without `@elyra/runtime` (a raw `fetch`), send both
`x-elyra-token` and `x-elyra-client-id` yourself.

## Related

- [Commands](commands.md) · [Events](events.md) · [Codegen](codegen.md)
- [Wire format](wire-format.md)
