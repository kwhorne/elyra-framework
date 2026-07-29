# @elyra/runtime

The frontend half of the [Elyra Framework](https://elyracode.com/framework) bridge:
`invoke()`, `channel()`, and the wrappers behind the generated `api.*` facade.

```bash
npm install @elyra/runtime
```

```ts
import { invoke, channel } from "@elyra/runtime";

const greeting = await invoke<string>("greet", "World");
const unsubscribe = channel<number>("tick").subscribe((n) => console.log(n));
```

Everything talks MessagePack over the `elyra://localhost` custom protocol, with the
per-run IPC token attached automatically. See the
[frontend runtime docs](https://elyracode.com/framework) for the full surface:
commands, events, dialogs, clipboard, notifications, window control, store, cache,
storage, queue, sidecars, deep links, and the auto-update toast.

MIT licensed.
