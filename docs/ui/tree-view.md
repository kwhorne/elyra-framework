# TreeView

A tree of things inside things: folders, categories, a chart of accounts,
an organisation. Branches open and close, one row is chosen, and the
keyboard moves through it the way it does through the Finder's sidebar or
Explorer's folders. Beside a [SplitPane](split-pane.md), it's the list of
a list-and-record window.

<!-- example: tree-view/basic -->
```svelte
<script>
  import { Badge, TreeView } from "@elyra/ui";

  // A chart of accounts (NS 4102): classes, groups, accounts.
  const accounts = [
    {
      id: "3", label: "3 Sales revenue",
      children: [
        { id: "30", label: "30 Sales, taxable", children: [{ id: "3000", label: "3000 Sales, high rate", entries: 412 }, { id: "3020", label: "3020 Sales, medium rate", entries: 18 }] },
        { id: "31", label: "31 Sales, exempt", children: [{ id: "3100", label: "3100 Sales, exempt", entries: 7 }] },
      ],
    },
    {
      id: "6", label: "6 Operating costs",
      children: [
        { id: "63", label: "63 Premises", children: [{ id: "6300", label: "6300 Rent", entries: 12 }] },
        { id: "68", label: "68 Office", children: [{ id: "6800", label: "6800 Office supplies", entries: 64 }, { id: "6860", label: "6860 Courses", entries: 3 }] },
      ],
    },
    { id: "8", label: "8 Financial items", can: "ledger.close" },
  ];

  let selected = $state("3000");
  let expanded = $state(["3", "30"]);
</script>

<div class="layout">
  <TreeView nodes={accounts} bind:selected bind:expanded label="Chart of accounts">
    {#snippet item(node)}
      <span>{node.label}</span>
      {#if node.entries}<Badge>{node.entries}</Badge>{/if}
    {/snippet}
  </TreeView>
  <p class="chosen">Account <strong>{selected}</strong> · {expanded.length} open</p>
</div>

<style>
  .layout { display: grid; gap: 8px; width: min(100%, 340px); }
  .chosen { margin: 0; color: var(--elyra-muted); font-size: 12px; }
</style>
```

- **A node** is `{ id, label }`, and anything else of yours. `children`
  makes it a branch. The `id` names it in `selected` and `expanded`, so
  ids must be unique in the whole tree, not only among siblings.
- **`selected`** is the chosen node's id, and **`onselect(node)`** is
  called when the user chooses one: a click, Enter or Space. A node with
  `disabled: true` shows, and can be reached, but isn't chosen.
- **`expanded`** is the ids of the open branches. Bind it to open some
  from the start, or to know which are open.
- **`can`** on a node hides it, and everything under it, from a user
  without that ability, as `can` does on a [NavItem](app-shell.md). Here
  "8 Financial items" needs `ledger.close`, which the reader doesn't have.
- **`item`** draws a row's content: an icon, a count, a status. Without
  it, a row is its `label`. The label is still what the type-ahead
  searches.

## Children read when a branch opens

A tree too big to read at once reads each branch's children the first
time it opens. Give the node `hasChildren: true` instead of `children`,
and the tree `load(node)`:

<!-- example: tree-view/lazy -->
```svelte
<script>
  import { TreeView } from "@elyra/ui";

  // In an app: `(node) => api.folders_children(node.id)`, an array or a Page<T>.
  const folders = {
    clients: [{ id: "acme", label: "Acme AS", hasChildren: true }, { id: "kunde", label: "Kunde AS", hasChildren: true }],
    acme: [{ id: "acme-2026", label: "2026" }, { id: "acme-2025", label: "2025" }],
    kunde: [],
  };
  const load = (node) => new Promise((resolve) => setTimeout(() => resolve(folders[node.id] ?? []), 600));

  const roots = [
    { id: "clients", label: "Clients", hasChildren: true },
    { id: "templates", label: "Templates", children: [{ id: "invoice", label: "Invoice" }, { id: "reminder", label: "Reminder" }] },
  ];
</script>

<div class="box">
  <TreeView nodes={roots} {load} persist="docs-folders" label="Folders" />
</div>

<style>
  .box { width: min(100%, 300px); }
</style>
```

- **`load`** returns the children, as an array or a `Page<T>` (its
  `data`), so a paginated command such as `api.folders_children(id)`
  works as it is. Its children can have `hasChildren` too.
- **Read once.** Closing a branch and opening it again doesn't read it
  again. "Loading…" shows under it while it's read.
- **When it fails** (offline, say), the branch says so, with "Try again".
  → on the branch tries again too.
- **Remembered open:** with `persist`, a lazy branch that was open when the
  window closed is read again when the window opens.

## Keeping what's open

`persist="key"` keeps `expanded` in the app's
[settings store](../store.md), under `elyra-ui.tree.<key>`. The next time
the window opens, the same branches are open. The example above keeps its
own. Open a branch and reload the page to see it. Give each tree its own
key.

## Keyboard

The tree is one stop in the Tab order: the chosen row, or the first. In
the tree:

| Key | Does |
|---|---|
| ↓ / ↑ | The next or previous row that shows |
| → | Opens a closed branch; on an open one, moves to its first child |
| ← | Closes an open branch; otherwise moves to the parent |
| Home / End | The first or last row that shows |
| Enter / Space | Chooses the row |
| `*` | Opens every branch beside the row |
| A letter | The next row starting with it; typed quickly, a word moves to the row it starts |

A click chooses a row. A click on the chevron, or a double click on the
row, opens or closes a branch.

## Accessibility

- It's the WAI-ARIA tree: `role="tree"`, named by `label`, with each row
  a `treeitem` and each branch's children in a `role="group"`.
- Each row has `aria-level`, `aria-setsize` and `aria-posinset`, so a
  screen reader says "Oslo, 1 of 2, level 3". A branch has
  `aria-expanded`, the chosen row has `aria-selected="true"`, a disabled
  one `aria-disabled`, and a branch being read `aria-busy`.
- A row is named by its own content, not by the rows under it.
- One row at a time is focusable (a roving `tabindex`), so Tab moves past
  the tree in one step.

## API

### TreeView

| Prop | Type | Default | Description |
|---|---|---|---|
| `nodes` | `Node[]` | `[]` | The top-level nodes: `{ id, label, children?, hasChildren?, can?, disabled? }`, and your own fields. |
| `selected` (bindable) | `string \| number \| null` | `null` | The chosen node's id. |
| `expanded` (bindable) | `(string \| number)[]` | `[]` | The open branches' ids. |
| `load` | `(node) => Promise<Node[] \| Page<Node>>` | — | Reads a `hasChildren` node's children, the first time it opens. |
| `persist` | `string` | — | A key to keep `expanded` under, in the settings store. |
| `label` | `string` | — | The tree's name for a screen reader. |
| `onselect` | `(node) => void` | — | Called when the user chooses a node. |
| `labels` | `{ loading?, failed?, retry? }` | `{}` | Its words, over the English: "Loading…", "Couldn't load this.", "Try again". |
| `class` | `string` | `""` | Added to the tree. |
| `item` | `Snippet<[Node]>` | — | A row's content; its `label` by default. |

Other attributes go to the `<ul role="tree">`.
