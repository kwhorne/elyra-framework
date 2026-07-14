# Single-instance & deep-linking

Two related features for apps that should behave like a single, link-aware
program. Both are core (no feature flag).

## Single-instance

```rust
App::new().single_instance().run()?;
```

The first process becomes the **primary**. Any later launch hands its command
line to the primary (over a loopback-only TCP rendezvous, guarded by a
per-app handshake) and exits immediately. The primary's window is raised and the
payload is delivered on `elyra:second-instance`:

```ts
import { onSecondInstance } from "@elyra/runtime";

onSecondInstance((payload) => {
  // e.g. a forwarded deep-link URL, or "" for a bare re-launch
});
```

If the rendezvous port is held by an unrelated process, enforcement is skipped
and the app runs normally.

## Deep-linking (custom URL scheme)

```rust
App::new().deep_link("myapp").run()?;   // handles myapp://…
```

- **Launch URL** — read it once on startup:
  ```ts
  import { deepLink, onDeepLink } from "@elyra/runtime";
  const url = await deepLink.initial();          // "myapp://…" | null
  onDeepLink((url) => { /* URLs delivered while running */ });
  ```
- **Delivery while running** — on macOS the OS open-URL event is forwarded to
  `elyra:deep-link`; on Windows/Linux the OS starts a new process with the URL,
  which [single-instance](#single-instance) forwards to the primary (also
  re-emitted on `elyra:deep-link`). Pair `deep_link` with `single_instance` for
  the best behavior.

### Registration

`deep_link` registers the scheme at startup (idempotent):

- **Windows** — `HKCU\Software\Classes\<scheme>` pointing at the executable.
- **Linux** — a `.desktop` entry with `MimeType=x-scheme-handler/<scheme>` plus
  `xdg-mime default`.
- **macOS** — registration belongs in the bundle's `Info.plist`; the runtime
  only handles incoming URLs. Add to your `.app`:

  ```xml
  <key>CFBundleURLTypes</key>
  <array><dict>
    <key>CFBundleURLName</key><string>com.example.myapp</string>
    <key>CFBundleURLSchemes</key><array><string>myapp</string></array>
  </dict></array>
  ```

> Registration writes to the OS (registry / desktop entries) and, on macOS,
> depends on packaging. Argv delivery and the macOS open-URL path are the parts
> exercised here; the registration side-effects are best verified on each target
> in a real install.

## Related

- [Windows](windows.md) · [Events](events.md) · [Configuration](configuration.md)
