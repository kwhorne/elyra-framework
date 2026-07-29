# Logging

`elyra::log` is the framework's `Log` facade: levels, targets, timestamps, and an
optional rotating file sink.

```rust
use elyra::{info, warn, error};

info!("sync finished in {}ms", elapsed);
warn!(target: "sync", "retrying: {e}");
error!("import failed: {e}");
```

## Configuration

```rust
use elyra::log::{Level, LogProvider};

App::new().provider(
    LogProvider::new()
        .level(Level::Debug)
        .to_app_dir("My App")      // <config>/<app>/app.log
        .rotate(5 * 1024 * 1024, 3) // 5 MiB, keep 3 files
)
```

`ELYRA_LOG` overrides the level at runtime (`error`/`warn`/`info`/`debug`/`trace`,
or `off`) — the same reflex as `RUST_LOG`.

`elyra::log::log_path()` returns the active file, which is what a "send us your
log" button should attach.

## What the framework logs

- every command dispatch at `debug`: name, byte sizes, duration
- command failures at `warn`, panics at `error` (with the panic message)
- tray/shortcut/updater setup problems at `warn`/`error`
- migrations and seeding at `info`

## Related

- [Commands](commands.md) · [Configuration](configuration.md)
