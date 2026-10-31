# SplitPane

Two panes with a divider between them that the user drags: a list beside
its record, an editor above its preview. The first pane has a size in
pixels and the second takes the rest. The divider moves from the keyboard
too, and the pane can keep its size in the app's settings store.

<!-- example: split-pane/basic -->
```svelte
<script>
  import { Badge, SplitPane } from "@elyra/ui";

  const customers = [
    { id: 1, name: "Ada Lovelace", city: "London", open: 2 },
    { id: 2, name: "Grace Hopper", city: "Arlington", open: 0 },
    { id: 3, name: "Kristen Nygaard", city: "Oslo", open: 1 },
  ];
  let chosen = $state(customers[0]);
</script>

<!-- The pane fills what it's in; here it's a box. -->
<div class="window">
  <SplitPane size={200} min={140} max={320} persist="docs-customers">
    {#snippet first()}
      <ul class="list">
        {#each customers as c (c.id)}
          <li><button class:active={c === chosen} onclick={() => (chosen = c)}>{c.name}</button></li>
        {/each}
      </ul>
    {/snippet}
    {#snippet second()}
      <article class="record">
        <h3>{chosen.name}</h3>
        <p>{chosen.city}</p>
        <Badge variant={chosen.open ? "warning" : "success"}>{chosen.open} open invoices</Badge>
      </article>
    {/snippet}
  </SplitPane>
</div>

<style>
  .window { width: 100%; height: 220px; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden; }
  .list { list-style: none; margin: 0; padding: 6px; }
  .list button {
    width: 100%; padding: 6px 8px; border: 0; border-radius: var(--elyra-radius);
    background: transparent; color: var(--elyra-text); font: inherit; text-align: left; cursor: pointer;
  }
  .list button.active { background: var(--elyra-accent-soft); }
  .record { padding: 16px; }
  .record h3 { margin: 0 0 4px; }
  .record p { margin: 0 0 12px; color: var(--elyra-muted); }
</style>
```

- **`first` and `second`** are snippets: the panes' content. Each pane
  scrolls on its own.
- **`bind:size`** is the first pane's size, in pixels. A double-click on the
  divider puts back the size it started with.
- **`min` and `max`** bound the first pane. **`minSecond`** is the room the
  second always keeps. When the window gets too small for both, the first
  pane gives way.
- The SplitPane fills what it's in (`width` and `height` 100%): give that a
  height, or put it in the [AppShell](app-shell.md)'s page.

## Stacked

`direction="vertical"` puts the second pane under the first. `size` is then
the first pane's height. `label` names the divider for a screen reader.

<!-- example: split-pane/vertical -->
```svelte
<script>
  import { SplitPane } from "@elyra/ui";

  let note = $state("Dear Ada,\n\nThe invoice for October is attached.");
</script>

<div class="window">
  <SplitPane direction="vertical" size={110} min={60} minSecond={60} label="Resize the preview">
    {#snippet first()}
      <textarea bind:value={note} aria-label="The note"></textarea>
    {/snippet}
    {#snippet second()}
      <div class="preview">{note}</div>
    {/snippet}
  </SplitPane>
</div>

<style>
  .window { width: 100%; height: 240px; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden; }
  textarea {
    width: 100%; height: 100%; box-sizing: border-box; padding: 12px; border: 0; resize: none;
    background: var(--elyra-inset); color: var(--elyra-text); font: 13px var(--elyra-font-mono);
  }
  .preview { padding: 12px; white-space: pre-wrap; color: var(--elyra-text); }
</style>
```

## Keeping the size

With `persist="customers"`, the size is kept in the app's
[settings store](../store.md) (under `elyra-ui.split.customers`) when the
user lets go of the divider, or stops pressing its keys. The next time the
pane is shown, in this run or the next, it starts from there. Give each
SplitPane its own key.

Without the shell (in a browser, in the tests), there's no store, and the
pane starts from `size` each time.

## Keyboard

The divider is in the tab order.

| Key | |
|---|---|
| <kbd>←</kbd> <kbd>→</kbd> | Side by side: makes the first pane narrower or wider, by `step` (16 px). |
| <kbd>↑</kbd> <kbd>↓</kbd> | Stacked: makes the first pane shorter or taller. |
| <kbd>Shift</kbd> + arrow | Four steps at once. |
| <kbd>Home</kbd> <kbd>End</kbd> | The smallest and the largest the first pane can be. |
| <kbd>Enter</kbd> | The size it started with. |

## Accessibility

- The divider is a focusable `role="separator"`, the window-splitter
  pattern. It has `aria-controls` (the first pane), `aria-orientation`
  (`vertical` between panes side by side), and `aria-valuenow`,
  `aria-valuemin` and `aria-valuemax` in pixels.
- Its line is 1 px, but it can be grabbed 3 px either side.

## API

### SplitPane

| Prop | Type | Default | Description |
|---|---|---|---|
| `size` (bindable) | `number` | `280` | The first pane's size, in pixels. |
| `direction` | `"horizontal" \| "vertical"` | `"horizontal"` | Side by side, or stacked. |
| `min` | `number` | `120` | The first pane's smallest size. |
| `max` | `number` | `Infinity` | The first pane's largest size. |
| `minSecond` | `number` | `120` | The room the second pane keeps. |
| `step` | `number` | `16` | How far an arrow key moves the divider. |
| `persist` | `string` | — | A key to keep the size under, in the settings store. |
| `label` | `string` | `"Resize"` | The divider's name. |
| `class` | `string` | `""` | Added to the outer element. |
| `first` | `Snippet` | — | The first pane. |
| `second` | `Snippet` | — | The second pane. |

Other attributes go to the outer element.
