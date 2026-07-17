# Storage

An ergonomic filesystem **storage** facade — the desktop-side counterpart to
[Askr](https://github.com/kwhorne/askr)/Laravel's `Storage::` (local disk). One
ecosystem: the same `put` / `get` / `exists` / `delete` / `url` surface in both
worlds, over a local directory here.

Every path is **jailed** to the disk root — `..`, absolute paths, and drive
prefixes are rejected, so callers can't escape the root.

Add the provider with a root directory:

```rust
use elyra::App;
use elyra::storage::StorageProvider;

App::new()
    .provider(StorageProvider::at("/path/to/app-data")) // e.g. from paths()
    .run()?;
```

Prefer an explicit, app-owned directory (the OS data dir, or a folder the user
picked). `StorageProvider::default()` roots at `./storage`.

## Rust API

```rust
use elyra::storage::Storage;

#[command]
async fn export(ctx: Ctx, csv: String) -> Result<String, String> {
    let disk = ctx.get::<Storage>();
    disk.put_str("exports/report.csv", &csv).map_err(|e| e.to_string())?;
    disk.url("exports/report.csv").map_err(|e| e.to_string())   // file:// URL
}
```

Full surface:

```rust
disk.put("a/b.bin", &bytes)?;      disk.put_str("a/b.txt", "hi")?;
disk.get("a/b.bin")?;              disk.get_str("a/b.txt")?;
disk.append("log.txt", b"line\n")?;
disk.exists("a/b.txt");
disk.delete("a/b.txt")?;           // missing is ok
disk.size("a/b.txt")?;
disk.make_directory("nested/dir")?;
disk.files("a")?;                  // file names in a dir (non-recursive)
disk.path("a/b.txt")?;             // absolute, jailed PathBuf
disk.url("a/b.txt")?;              // file:// URL
```

## Frontend API (`@elyra/runtime`)

Text content (use the Rust `Storage` for binary):

```ts
import { storage } from "@elyra/runtime";

await storage.put("notes/today.md", "# Notes");
const text = await storage.get("notes/today.md");
const names = await storage.files("notes");   // ["today.md", …]
const link = await storage.url("notes/today.md");
```

## Storage vs Store vs Database

| Use | Reach for |
| --- | --- |
| Files / blobs on disk | **Storage** (this) |
| Small durable settings (key-value) | [`Store`](store.md) |
| Structured, queryable data | [`database`](database.md) |

## Related

- [System integration](system.md) — `paths()` for good root directories.
- [Cache](cache.md) · [Container & providers](container-and-providers.md)
