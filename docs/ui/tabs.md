# Tabs

Views of one thing, one at a time: a customer's details, invoices and notes.

<!-- example: tabs/basic -->
```svelte
<script>
  import { Callout, Empty, Tabs } from "@elyra/ui";

  let tab = $state("invoices");
  const TABS = [
    { id: "details", label: "Details" },
    { id: "invoices", label: "Invoices" },
    { id: "notes", label: "Notes" },
    { id: "audit", label: "Audit", can: "customers.delete" },
    { id: "files", label: "Files", disabled: true },
  ];
</script>

<Tabs bind:value={tab} tabs={TABS} label="Customer">
  {#snippet panel(id)}
    {#if id === "invoices"}
      <Callout variant="danger" title="2 invoices overdue">INV-2041 and INV-2047, 1 860 € in all.</Callout>
    {:else if id === "notes"}
      <Empty title="No notes yet" />
    {:else}
      <p>Ada Lovelace, Analytical Engines Ltd, London.</p>
    {/if}
  {/snippet}
</Tabs>
```

`tabs` are `{ id, label, can, disabled }`. `bind:value` is the shown tab's
`id`, and `panel(id)` is the snippet that draws it. A tab with `can` is
hidden from a user without the ability; here, the user can't see "Audit". A
`disabled` tab is shown and skipped. When `value` isn't a tab the user can
see, the first one that isn't disabled is shown.

## Keyboard

| Key | |
|---|---|
| <kbd>Tab</kbd> | Onto the shown tab, then into the panel. |
| <kbd>←</kbd> <kbd>→</kbd> | The previous or the next tab, round from the end, and shows it. |
| <kbd>Home</kbd> <kbd>End</kbd> | The first tab, the last tab. |

## Accessibility

- A `role="tablist"`, named by `label`. Each tab is a `role="tab"` with
  `aria-selected` and `aria-controls`.
- Only the shown tab is in the <kbd>Tab</kbd> order. The arrow keys move
  between the tabs.
- The panel is a `role="tabpanel"`, labelled by its tab, and it takes focus.

## API

### Tabs

| Prop | Type | Default | Description |
|---|---|---|---|
| `tabs` | `Array<{ id, label, can?, disabled? }>` | `[]` | |
| `value` (bindable) | `string` | — | The shown tab's `id`. |
| `label` | `string` | — | The tab list's name. |
| `class` | `string` | `""` | |
| `panel` | `Snippet<[id]>` | — | Draws the shown tab. |
