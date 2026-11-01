# UI components

`@elyra/runtime` ships themed, dependency-free UI primitives so apps don't have
to reinvent them (or fall back to the webview's unstyled `window.confirm`). They
read Elyra's design tokens — `--elyra-raised`, `--elyra-bg`, `--elyra-text`,
`--elyra-muted`, `--elyra-accent`, `--elyra-border`, `--elyra-danger`, … from
`@elyra/ui/tokens.css` ([RFC 0012](proposals/0012-elyra-ui.md)) — and, for an
app on the names used before 0.21, `--surface`/`--panel`, `--bg`, `--text`,
`--muted`/`--text-dim`, `--accent` and `--border`, with dark defaults. So they
match the app's theme, light or dark, out of the box.

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
**Esc** to close. The ⌘K binding is only active once commands or searches
are registered, or once the app's assistant has been used.

### Searching the app's records

A search source is asked as the user types, and its results come after the
commands, under its title. Typing `Ada` finds the command and the customer:

```ts
import { navigate, registerSearch } from "@elyra/runtime";
import { api } from "./bindings";

const off = registerSearch({
  id: "customers",
  title: "Customers",
  can: "customers.view",           // not asked without it
  search: async (query, { signal }) => {
    const page = await api.customers_index({ search: query, per_page: 5 });
    return page.data.map((c) => ({
      title: c.name,
      subtitle: "Customer",
      action: () => navigate(`/customers/${c.id}`),
    }));
  },
});
```

- **When:** once typing settles (150 ms), from 2 characters (`minChars`).
  Only the latest query's answers are shown, and a newer one aborts the
  older one's `signal`.
- **Who:** a source with `can` is asked only when `$can` allows it, and is
  asked as soon as the user's abilities say so.
- **Trouble:** a source that fails is left out; the others still show.
- **`make:resource --view`** registers one for each resource with a name to
  show it by, through `resources/index.js`. Its `can` is the resource's
  `.view` ability when the views are gated by the backend's abilities.
- **`search(query)`** asks every source the way the palette does, for a
  search page of the app's own: `[{ id, title, results }]`.

With the app's assistant (`App::assistant`), a query that matches no command
or record offers **Ask the assistant: “…”**, which opens its panel with that
question.

The palette is a dialog with a combobox and a listbox: the active result is
the input's `aria-activedescendant`, and each source's results are a group
named by its title.

## Assistant panel

The app's assistant ([RFC 0011](proposals/0011-in-app-assistant.md)) has a
built-in panel on the right of the window:

```svelte
<script>
  import { openAssistant } from "@elyra/runtime";
</script>

<button onclick={() => openAssistant()}>Ask</button>
<!-- or with a question: openAssistant("Which invoices are overdue?") -->
```

- It shows the conversation, each command the assistant runs (✓ or failed),
  and the answer. The model's words are always shown as text.
- A command whose ability `Assistant::confirm` names asks first, in the
  themed dialog, with its description and arguments: **Allow** or **Don't**.
- **Stop** stops the turn, **New** starts a new conversation, and **Esc** or
  **Close** closes the panel; the conversation stays.
- Without a model (`/__assistant/source` is `none`), it asks for the user's
  own Anthropic key and keeps it in the keychain. **Forget my key** removes it.

For a panel of your own, `assistant` is a Svelte store of the conversation:

```svelte
<script>
  import { assistant } from "@elyra/runtime";
  let text = $state("");
</script>

{#each $assistant.items as item}
  {#if item.kind === "user" || item.kind === "answer"}<p>{item.text}</p>
  {:else if item.kind === "tool"}<small>{item.tool} {item.ok === null ? "…" : item.ok ? "✓" : "failed"}</small>
  {:else if item.kind === "error"}<p class="error">{item.text}</p>{/if}
{/each}
<input bind:value={text} /> <button onclick={() => assistant.ask(text)} disabled={$assistant.busy}>Ask</button>
```

`assistant.ask(text)`, `.stop()`, `.reset()`, `.tools()`, `.source()` and
`.setKey(key | null)` (write-only) talk to `/__assistant/*`. Its questions
still use the themed dialog. `assistant.selected(kind, ids)` says what's
selected in the window, for the assistant's
[note of the window](assistant.md#what-it-sees-of-the-window).

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
- [The app's assistant](proposals/0011-in-app-assistant.md) · [Its Laravel side](backend.md#the-apps-assistant)
- [Frontend runtime](frontend-runtime.md)
