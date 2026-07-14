# UI components

`@elyra/runtime` ships themed, dependency-free UI primitives so apps don't have
to reinvent them (or fall back to the webview's unstyled `window.confirm`). They
read your app's CSS variables — `--surface`/`--panel`, `--bg`, `--text`,
`--muted`, `--accent`, `--border` — with dark fallbacks, so they match the
scaffold's Grove theme and the built-in [About](about.md) / [update](updater.md)
components out of the box.

## Dialogs

Promise-based replacements for `alert` / `confirm` / `prompt`:

```ts
import { alert, confirm, prompt } from "@elyra/runtime";

await alert("Saved.", { title: "Done" });

if (await confirm("Delete this item?", { danger: true, confirmLabel: "Delete" })) {
  // ...
}

const name = await prompt("Project name", { defaultValue: "untitled" });
if (name !== null) { /* user confirmed */ }
```

- `confirm` → `Promise<boolean>`, `prompt` → `Promise<string | null>`, `alert` → `Promise<void>`.
- **Enter** activates the primary button, **Esc** / overlay-click cancels.
- `danger: true` styles the primary button as destructive.

## Toasts

In-app notifications (distinct from OS notifications via [`notify`](system.md)):

```ts
import { toast } from "@elyra/runtime";

toast("Copied to clipboard");
toast("Export finished", { variant: "success" });
const t = toast("Uploading…", { duration: 0 }); // sticky
t.dismiss();
```

Variants: `"info"` (default), `"success"`, `"error"`. `duration` is milliseconds
(default `3500`; `0` keeps it until clicked). Toasts stack bottom-right.

## Command palette (⌘K)

Register commands once; the palette opens on **⌘K / Ctrl-K** automatically:

```ts
import { registerCommands, openCommandPalette } from "@elyra/runtime";

registerCommands([
  { id: "new", title: "New file", subtitle: "Create a file", keywords: "add create", action: () => {} },
  { id: "about", title: "About this app", action: () => openAbout() },
]);

// or open it yourself (e.g. from a button):
openCommandPalette();
```

Type to filter (title + subtitle + keywords), **↑/↓** to move, **Enter** to run,
**Esc** to close. The ⌘K binding is only active once commands are registered.

## Context menu

Show a menu at the pointer from a right-click handler:

```svelte
<script>
  import { contextMenu } from "@elyra/runtime";
  function onMenu(e) {
    contextMenu(e, [
      { label: "Rename", action: rename },
      { label: "Duplicate", action: duplicate },
      { separator: true },
      { label: "Delete", action: remove, disabled: !selected },
    ]);
  }
</script>

<div oncontextmenu={onMenu}>…</div>
```

`contextMenu` calls `preventDefault()`, keeps itself on-screen, and closes on
select / click-away / `Esc`.

## Related

- [About dialog](about.md) · [Auto-updater toast](updater.md) · [System integration](system.md)
- [Frontend runtime](frontend-runtime.md)
