# Autostart (launch at login)

Feature-gated behind `autostart`. Registers the app to start when the user logs
in — LaunchAgents on macOS, the registry `Run` key on Windows, and a `.desktop`
autostart entry on Linux (via the `auto-launch` crate).

```toml
elyra = { version = "0.5", features = ["autostart"] }
```

## Frontend API

```ts
import { autostart } from "@elyra/runtime";

if (!(await autostart.isEnabled())) {
  await autostart.enable();
}
await autostart.disable();
```

## Rust API

```rust
use elyra::autostart;

autostart::enable(&app_name)?;      // app_name is usually your About name
let on = autostart::is_enabled(&app_name)?;
```

The entry points at the current executable and is keyed by the
[About](about.md) name.

## Caveats

- On **macOS**, reliable behavior generally requires a bundled `.app` (a bare
  `cargo run` binary may not persist correctly across sessions).
- Enabling/disabling touches OS state (registry / LaunchAgents / autostart dir);
  a failure rejects the promise.

## Related

- [Configuration](configuration.md) · [Windows](windows.md)
