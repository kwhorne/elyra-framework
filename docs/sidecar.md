# Sidecar processes

Feature-gated behind `sidecar`. Spawn and manage helper child processes: their
`stdout`/`stderr` lines and exit are streamed to the frontend, and you can write
to `stdin` or kill them.

```toml
elyra = { version = "0.4", features = ["sidecar"] }
```

No extra crate is pulled in — the feature enables the needed `tokio` bits.

## Frontend API

```ts
import { sidecar, onSidecar } from "@elyra/runtime";

const off = onSidecar((e) => {
  if (e.kind === "data") console.log(`[${e.stream}] ${e.line}`);
  if (e.kind === "exit") console.log("exited with", e.code);
});

const id = await sidecar.spawn("my-helper", ["--serve", "8080"]);
await sidecar.write(id, "input line\n");
await sidecar.kill(id);
```

Each event carries the sidecar `id`, a `kind` (`"data"` | `"exit"`), and for
output the `stream` (`"stdout"` | `"stderr"`) + `line`, or for exit the `code`.

## Rust API

Also available in the container as [`Sidecar`](../framework/src/sidecar.rs):

```rust
use elyra::sidecar::Sidecar;

#[command]
async fn start_helper(ctx: Ctx) -> Result<u32> {
    Ok(ctx.get::<Sidecar>().spawn("my-helper", &["--serve".into()])?)
}
```

Output is line-buffered (one line = one event). To ship a helper binary
alongside your app, place it next to the executable (see `paths().exe` in
[system integration](system.md)) and spawn it by absolute path.

## Related

- [System integration](system.md) · [Events](events.md)
