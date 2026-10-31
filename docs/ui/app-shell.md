# AppShell

The window. A toolbar with the app's name and its actions runs along the
top. The navigation runs down the side, the page fills the middle, and the
[StatusBar](status-bar.md) runs along the bottom. Put the app's router
inside it.

<!-- example: app-shell/basic -->
```svelte
<script>
  import { AppShell, Button, Empty } from "@elyra/ui";

  // In an app: `import { nav } from "./resources/index.js"`, which
  // make:resource keeps.
  const nav = [
    { label: "Overview", path: "/" },
    { label: "Customers", path: "/customers", count: 3 },
    { label: "Invoices", path: "/invoices", can: "invoices.view" },
    { label: "Settings", path: "/settings" },
  ];
</script>

<!-- The shell fills the window; here it's in a box. -->
<div class="window">
  <AppShell title="CRM" {nav}>
    {#snippet actions()}
      <Button size="sm" variant="primary" href="/customers/new">New customer</Button>
    {/snippet}
    <Empty title="Pick a customer" description="Or press ⌘K and type a name." />
  </AppShell>
</div>

<style>
  .window { width: 100%; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden; }
  .window :global(.elyra-app-shell) { height: 340px; }
</style>
```

`nav` is a list of `{ label, path, count, can }`. That's the shape of the
resources' registry `make:resource` keeps in `src/resources/index.js`:

```svelte
<script>
  import { AppShell } from "@elyra/ui";
  import Router from "./Router.svelte";
  import { routes } from "./routes.js";
  import { nav } from "./resources/index.js";
</script>

<AppShell title="CRM" {nav}>
  <Router {routes} />
</AppShell>
```

A link is current on its page and the pages under it, so `/customers` is
current on `/customers/7`. A link with `can` is hidden from a user without
the ability. A `count` shows next to it.

## Across the top

`layout="topbar"` puts the navigation in the toolbar. To draw the links
yourself, put `NavItem`s in a `navigation` snippet instead of passing `nav`.
`statusBar={false}` leaves the status bar out.

<!-- example: app-shell/topbar -->
```svelte
<script>
  import { AppShell, NavItem, Skeleton } from "@elyra/ui";
</script>

<div class="window">
  <AppShell title="Notes" layout="topbar" statusBar={false}>
    {#snippet navigation()}
      <NavItem path="/notes">All notes</NavItem>
      <NavItem path="/customers">Customers</NavItem>
      <NavItem path="/archive">Archive</NavItem>
    {/snippet}
    <Skeleton lines={4} />
  </AppShell>
</div>

<style>
  .window { width: 100%; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden; }
  .window :global(.elyra-app-shell) { height: 220px; }
</style>
```

## The parts

`Toolbar`, `Sidebar` and `NavItem` are there on their own, for a window laid
out another way, or a second window.

<!-- example: app-shell/parts -->
```svelte
<script>
  import { writable } from "svelte/store";
  import { Badge, Button, NavItem, Separator, Sidebar, Toolbar } from "@elyra/ui";

  // A count can be a store: the unread notifications, say.
  const unread = writable(4);
</script>

<div class="window">
  <Toolbar title="CRM">
    {#snippet actions()}
      <Button size="sm" onclick={() => unread.update((n) => n + 1)}>Notify</Button>
    {/snippet}
  </Toolbar>
  <Sidebar>
    {#snippet header()}<Badge variant="accent">Analytical Engines</Badge>{/snippet}
    <NavItem path="/inbox" count={unread}>Inbox</NavItem>
    <NavItem path="/customers">
      {#snippet icon()}●{/snippet}
      Customers
    </NavItem>
    <NavItem path="/invoices" can="customers.delete">Hidden: no ability</NavItem>
    <Separator />
    <NavItem path="/settings">Settings</NavItem>
    {#snippet footer()}<small>Signed in as Ada</small>{/snippet}
  </Sidebar>
</div>

<style>
  .window {
    display: grid; grid-template-rows: auto 260px; width: 100%;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden;
  }
</style>
```

- **NavItem** links to an app `path`, through the router. `count` is a
  number, or a store of one (the unread notifications). It shows when it's
  more than 0, and as "99+" past 99. `icon` is a snippet before the label.
- **Sidebar** holds the `NavItem`s, with `header` and `footer` snippets. It
  is 220px wide.
- **Toolbar** has a `title`, what you put in it, and `actions` on the right.

The AppShell's own status bar takes no children. For one with the app's own
items, lay the window out from the parts, with a [StatusBar](status-bar.md)
of your own.

## Accessibility

- The page is the `<main>`, the toolbar a `<header>`, and the navigation a
  `<nav>` named "Main".
- The current link has `aria-current="page"`.
- A NavItem is a link, so the keyboard reaches it with <kbd>Tab</kbd> and
  opens it with <kbd>Enter</kbd>.

## API

### AppShell

| Prop | Type | Default | Description |
|---|---|---|---|
| `title` | `string` | — | The app's name, in the toolbar. |
| `nav` | `Array<{ label, path, count?, can? }>` | `[]` | The navigation. |
| `layout` | `"sidebar" \| "topbar"` | `"sidebar"` | Where the navigation is. |
| `statusBar` | `boolean` | `true` | Whether there's a status bar. |
| `class` | `string` | `""` | |
| `actions` | `Snippet` | — | The toolbar's buttons, on the right. |
| `navigation` | `Snippet` | — | `NavItem`s of your own, instead of `nav`. |
| `children` | `Snippet` | — | The page. |

### Sidebar

| Prop | Type | Default | Description |
|---|---|---|---|
| `label` | `string` | `"Main"` | The navigation's name. |
| `class` | `string` | `""` | |
| `header` | `Snippet` | — | Above the links. |
| `footer` | `Snippet` | — | At the bottom. |
| `children` | `Snippet` | — | The `NavItem`s. |

### NavItem

| Prop | Type | Default | Description |
|---|---|---|---|
| `path` | `string` | — | **Required.** An app path: `/customers`. |
| `count` | `number \| Readable<number>` | — | Shown when more than 0. |
| `can` | `string` | — | The ability it needs; hidden without it. |
| `class` | `string` | `""` | |
| `icon` | `Snippet` | — | Before the label. |
| `children` | `Snippet` | — | The label. |

### Toolbar

| Prop | Type | Default | Description |
|---|---|---|---|
| `title` | `string` | — | |
| `class` | `string` | `""` | |
| `actions` | `Snippet` | — | On the right. |
| `children` | `Snippet` | — | After the title. |
