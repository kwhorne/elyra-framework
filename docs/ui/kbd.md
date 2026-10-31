# Kbd

A key, or a combination of keys, as the platform writes it: ⌘K on macOS and
Ctrl+K on Windows and Linux. `keys` is written the way Elyra's
[shortcuts](../shortcuts.md) and [menus](../menu.md) write accelerators
(`CmdOrCtrl+Shift+P`), so one string serves the shortcut and the hint.

<!-- example: kbd/basic -->
```svelte
<script>
  import { Kbd } from "@elyra/ui";
</script>

<!-- As this window's platform writes them: ⌘ on a Mac, Ctrl elsewhere. -->
<ul class="keys">
  <li><Kbd keys="CmdOrCtrl+K" /> Search everything</li>
  <li><Kbd keys="CmdOrCtrl+Shift+P" /> Run a command</li>
  <li><Kbd keys="Alt+Up" /> Move the line up</li>
  <li><Kbd keys={["Shift", "Enter"]} /> A new line in the message</li>
  <li><Kbd keys="Esc" /> Close</li>
</ul>

<style>
  .keys { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; color: var(--elyra-text); }
  .keys li { display: flex; align-items: center; gap: 12px; }
  .keys :global(.elyra-kbd) { min-width: 96px; }
</style>
```

- **`keys`** is a string joined by `+`, or a list (`["Shift", "Enter"]`).
  `CmdOrCtrl+Plus`, or a lone `+`, is the plus key.
- **Modifiers and named keys** are drawn as the platform draws them: ⌘ ⌃ ⌥
  ⇧ ↩ ⌫ ⇥ on macOS, and Ctrl, Alt, Shift, Enter, Backspace, Tab elsewhere.
  `Cmd` (or `Meta`, `Super`) is ⌘ on macOS and Win elsewhere. Arrows are
  arrows everywhere. A letter is upper case, and anything else is as
  written (`F5`).
- **Joined:** macOS writes a combination without a separator (⌘⇧P), and
  the others with `+` (Ctrl+Shift+P).

## The platform

Kbd asks the WebView which platform it's on. `platform="mac"` or
`platform="other"` sets it instead.

<!-- example: kbd/platforms -->
```svelte
<script>
  import { Kbd } from "@elyra/ui";
</script>

<!-- `platform` sets it, for a screenshot or a docs page about the other one. -->
<dl class="rows">
  <dt>macOS</dt>
  <dd><Kbd keys="CmdOrCtrl+Shift+E" platform="mac" /></dd>
  <dt>Windows and Linux</dt>
  <dd><Kbd keys="CmdOrCtrl+Shift+E" platform="other" /></dd>
</dl>

<style>
  .rows { display: grid; grid-template-columns: max-content auto; align-items: center; gap: 8px 16px; margin: 0; color: var(--elyra-text); }
  dd { margin: 0; }
</style>
```

## In a menu

A [`MenuItem`](dropdown.md) takes `shortcut`, drawn at its end with a Kbd.
It only shows the key: register the key itself in the
[app menu](../menu.md), as a [global shortcut](../shortcuts.md), or with a
`keydown` listener.

<!-- example: dropdown/shortcuts -->
```svelte
<script>
  import { Dropdown, MenuItem } from "@elyra/ui";
</script>

<Dropdown label="File">
  <MenuItem shortcut="CmdOrCtrl+N">New customer</MenuItem>
  <MenuItem shortcut="CmdOrCtrl+Shift+E">Export…</MenuItem>
  <MenuItem shortcut="CmdOrCtrl+Backspace" danger>Delete</MenuItem>
</Dropdown>
```

## Accessibility

- A Kbd is a `<kbd>` with a `<kbd>` for each key, the way HTML marks up a
  combination.
- A screen reader hears the keys' names ("Command Shift P", "Control Shift
  P"), not the symbols, which are hidden from it.
- In a `MenuItem`, the key is the item's `aria-keyshortcuts`
  (`Meta+Shift+E`), and not part of its name.

## API

### Kbd

| Prop | Type | Default | Description |
|---|---|---|---|
| `keys` | `string \| string[]` | `""` | `"CmdOrCtrl+K"`, or `["Shift", "Enter"]`. |
| `platform` | `"mac" \| "other"` | — | The platform to write it for. Without it, the WebView's. |
| `class` | `string` | `""` | Added to the outer `<kbd>`. |

Other attributes go to the outer `<kbd>`.
