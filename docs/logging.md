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

`elyra::log::log_path()` returns the active file. For a "send us your log"
button, [`App::diagnostics()`](diagnostics.md) is the better answer: one file
with the end of the log, the versions, the system and the connection, and no
secrets.

## Kept in memory, redacted

Whatever the sinks, the last 1,000 records are kept in memory:
`elyra::log::recent(n)` hands them out, and the [diagnostics](diagnostics.md)
use them when there's no log file.

Every record's message is redacted before it reaches stderr, the file or
memory: a bearer token, an `Authorization` header's value, the value of
`token=`, `password=`, `secret`, `signature=`, `api_key` and their kin, the
query string of a URL carrying a signature, the password in a URL
(`postgres://app:•••@db.internal`), and an Anthropic or OpenAI key logged
without a name (`sk-ant-…`, `sk-proj-…`) become `•••`.
`elyra::log::redact(text)` is the same function.

## What the framework logs

- every command dispatch at `debug`: name, byte sizes, duration
- command failures at `warn`, panics at `error` (with the panic message)
- tray/shortcut/updater setup problems at `warn`/`error`
- migrations and seeding at `info`
- the page's uncaught errors and unhandled rejections at `error`, with the
  target `page` (sent by `@elyra/runtime`, in every app)

## Related

- [Commands](commands.md) · [Configuration](configuration.md)
