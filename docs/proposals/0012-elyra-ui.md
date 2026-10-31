# RFC 0012 — `@elyra/ui`: desktop components that know the app

**Status:** implemented (2026-10-31) · **Target:** 0.21.0 · the guide: [docs/ui.md](../ui.md) · builds on [RFC 0001](0001-make-resource.md), [RFC 0002](0002-live-queries.md), [RFC 0005](0005-laravel-backend.md) and [RFC 0010](0010-laravel-abilities.md) · see [Decisions](#decisions)

## Summary

Elyra's backend half is complete: typed commands, live queries, Laravel's
validation, abilities, offline, files, an assistant. The screens on top are
still hand-written HTML with a few shared classes (`.btn`, `.card`). This RFC
adds a component library for Svelte 5, **`@elyra/ui`**, in the spirit of
Laravel's Flux. It has a small, careful API, good defaults, keyboard first,
and a calm look. It is written for desktop apps, and it knows the rest of
Elyra.

```svelte
<script>
  import { live } from "./bindings";
  import { Table, Button, Field, Input, StatusBar } from "@elyra/ui";

  let query = $state({ search: "", page: 1 });
  const customers = $derived(live.customers_index(query));
</script>

<Table page={customers} bind:query columns={["name", "email", "credit"]} sortable>
  {#snippet actions(row)}
    <Button size="sm" href={`/customers/${row.id}`}>Open</Button>
    <Button size="sm" variant="danger" can="customers.delete" confirm="Delete {row.name}?"
            onclick={() => api.customers_destroy(row.id)}>Delete</Button>
  {/snippet}
</Table>

<StatusBar />   <!-- online / offline, the outbox, transfers, the assistant -->
```

- **Svelte 5 components** with no other dependency, themed by one set of
  design tokens, dark and light.
- **They know Elyra.**
  - A `<Table>` takes a `live.*` store of a `Page<T>`, the shape
    `make:resource` lists already use.
  - A `<Field>` shows Laravel's `422` and `ValidationErrors` per field.
  - `can="…"` hides what the signed-in user can't do (RFC 0010).
  - `<StatusBar>` shows the connection, the outbox, transfers and the
    assistant.
- **A package, and yours when you need it.** `@elyra/ui` is updated with
  Elyra. `rata make:ui Table` copies a component into the app to change, the
  way `FileField.svelte` is written once and then is the app's.
- **The generated screens use it.** `make:resource --view` writes views from
  these components, so every app starts from the same quality, with far less
  generated markup.

## Motivation

- **Desktop apps are judged by their controls.** A list that can't be driven
  from the keyboard, a select that isn't searchable, or a dialog that loses
  focus all read as "a web page in a window". Avalonia and Flux look right
  because they ship real controls with real keyboard behaviour. Elyra should
  have them, in Svelte, without leaving the WebView that makes everything
  else possible.
- **Every app writes the same glue.**
  - Unpacking `validationErrors(err)` into fields.
  - Paging and sorting a `Page<T>`.
  - Hiding buttons with `$can`.
  - A banner for `$connection`.

  The framework knows these shapes, so the components should.
- **The generated views carry their own CSS.** Each `make:resource` view
  writes its styles inline, so a fix to the look means regenerating, and
  apps drift.
- **The theme has drifted already.**
  - The scaffold's `app.css` names `--panel` and `--text-dim`.
  - The runtime's dialogs, toasts, palette and assistant panel read
    `--surface` and `--muted`, with their own dark fallbacks.

  Two vocabularies for one theme.

## Current state (0.20)

- `rata new` writes `app/src/app.css`: the Tokyo Night palette as CSS
  variables (`--bg`, `--bg-2`, `--bg-3`, `--panel`, `--border`, `--text`,
  `--text-dim`, `--accent`, `--accent-2`, `--green`, `--red`, `--amber`), a
  light theme under `[data-theme="light"]`, and `.toolbar`, `.nav`, `.card`,
  `.btn`, `.row`, `input` styles.
- `@elyra/runtime` draws its built-ins in plain DOM: `confirm` / `alert` /
  `prompt`, `toast`, the ⌘K palette, `contextMenu`, the About dialog, the
  update toast and the assistant panel. It has no framework dependency.
- `make:resource --view` writes `Index.svelte`, `Form.svelte` and
  `Show.svelte` per resource, each with its own `<style>`, and
  `FileField.svelte` once.
- The stores a component would use exist:
  - `live.*` gives `Live<T>` (`{ value, error, loading }`).
  - `validationErrors(err)` gives the field-to-messages bag.
  - `can` and `abilities`, `connection`, `outbox`, `transfers` and
    `assistant` are all stores.

## Design

### The package

`@elyra/ui` is a second package in this repository (`ui/`), with `svelte`
as a peer dependency and `@elyra/runtime` as its only dependency. It ships
`.svelte` sources (compiled by the app's own Vite, as Svelte libraries do)
and is attached to each release as `elyra-ui-<version>.tgz`, beside the
runtime's. `rata new` adds it, and `rata new --elyra <path>` links it with
`file:`, as it does the runtime.

`@elyra/runtime` stays framework-free: an app in another framework, or none,
keeps everything it has today.

### Design tokens

One vocabulary, defined once, in `@elyra/ui/tokens.css`. The app's `app.css`
imports it and may override any token:

| Token | For | Replaces |
|---|---|---|
| `--elyra-bg`, `--elyra-surface`, `--elyra-raised` | window, panels, menus and popovers | `--bg`/`--bg-2`/`--bg-3`, `--panel`, `--surface` |
| `--elyra-text`, `--elyra-muted` | text, secondary text | `--text`, `--text-dim`, `--muted` |
| `--elyra-border`, `--elyra-focus` | lines, the focus ring | `--border` |
| `--elyra-accent`, `--elyra-accent-soft` | primary actions, selection | `--accent`, `--accent-2` |
| `--elyra-danger`, `--elyra-success`, `--elyra-warning` | states | `--red`, `--green`, `--amber` |
| `--elyra-radius`, `--elyra-space`, `--elyra-font`, `--elyra-font-mono` | shape, rhythm, type | — |

- **Dark and light.** Dark is the default, light comes under
  `[data-theme="light"]`, and `auto` follows the OS, as the scaffold's
  `theme.js` does today.
- **The old names stay as aliases** for a release, so existing apps keep
  their look.
- **The runtime reads the same tokens.** Its built-ins (dialogs, toasts, the
  palette, the assistant panel) switch to them, so a `confirm()` and a
  `<Modal>` look the same.

The look is Elyra's own, built on the palette apps already have. Flux is
the inspiration for restraint and craft, not something to copy: Flux is a
commercial product, and no markup, styles or icons are taken from it.

### Components (first set)

| Group | Components |
|---|---|
| Actions | `Button` (variants `primary` / `ghost` / `danger`, sizes, `loading`, `can`, `confirm`), `ButtonGroup`, `Dropdown` + `MenuItem` |
| Forms | `Form`, `Field`, `Input`, `Textarea`, `Select` (searchable), `Checkbox`, `Switch`, `Radio` |
| Data | `Table` (a `Page<T>` or a plain array), `Pagination`, `Badge`, `Empty` |
| Overlays | `Modal`, `Drawer`, `Tooltip`, `Popover` |
| Layout | `AppShell`, `Sidebar` + `NavItem`, `Toolbar`, `StatusBar`, `Tabs`, `Card`, `Separator` |
| Feedback | `Callout`, `Spinner`, `Skeleton` |

Later, once the first set is used in real apps: `DataGrid` (virtual
scrolling, column resize, cell editing), `Combobox` (async options),
`DatePicker`, `TreeView`, `SplitPane`, `Kbd`, `Avatar`.

Each component:

- **Works from the keyboard.** Tab order, arrow keys in lists, menus and
  tabs, Enter and Space, and Esc closes. Focus is trapped in a modal and
  restored when it closes.
- **Carries ARIA roles and states.** A `Select` is a combobox with a
  listbox, a `Table` sort header says its direction, and a `Switch` is
  `role="switch"`.
- **Takes `class` and passes the rest on**, so an app can adjust it without
  a fork.
- **Uses snippets for slots** (`{#snippet actions(row)}`), never named-slot
  APIs Svelte 5 is leaving behind.

### Knowing Elyra

This is what a general-purpose library can't do:

- **`can="ability"`** on `Button`, `MenuItem`, `NavItem` and `Tab` hides the
  control when the user lacks the ability (`$can`). `can-mode="disable"`
  shows it disabled instead. The server still decides; this is for not
  offering what it would refuse.
- **`<Form onsubmit>`** runs the submit, catches a `ValidationError` (a
  Laravel `422`, or `ValidationErrors` from Rust), and puts each message on
  the `<Field name="…">` it belongs to. Any other error goes in a `Callout`
  at the top of the form. While it runs, the form is busy.
- **`<Table page={live.x(query)} bind:query>`** shows a `Live<Page<T>>`.
  - Its loading state, error and empty state.
  - Sorting by the columns the command allows.
  - Search and paging written into `query`, so the live query re-runs.
  - A plain array works too, sorted and paged on the client.
- **`<StatusBar>`** shows `$connection` (offline since …), `$outbox` (3
  changes waiting, one refused), `$transfers` (one upload, 40 %) and, with an
  assistant, a button for `openAssistant()`. Each item can be turned off.
- **`<Button confirm="…">`** asks with the runtime's `confirm()` before
  `onclick`, and `danger` makes the dialog's button red.
- **`AppShell`** is the scaffold's toolbar, navigation and content, as a
  component that the router (`route`, `href`) and the resources' `nav`
  registry plug into.

### Owning a component: `rata make:ui`

`rata make:ui Table` copies `Table.svelte`, and the internal helpers it
uses, into `app/src/lib/ui/`. It prints the import to change:

```
wrote app/src/lib/ui/Table.svelte
import Table from "./lib/ui/Table.svelte";   // instead of "@elyra/ui", relative to the view
```

The copy is the app's: rata never overwrites it (`--force` to replace).
`rata make:ui --list` shows the components that can be copied.

### `make:resource --view`

The generated `Index.svelte`, `Form.svelte` and `Show.svelte` are built from
`@elyra/ui`:

- `Table`, `Form` and `Field`, and `Button can="…"` for each action.
- No per-view `<style>`.
- `FileField` becomes `@elyra/ui`'s `FileField`.

Views generated before stay as they are: they're the app's. The smoke harness
checks the new views as it checks today's.

### Testing

- **Unit tests in a DOM.** vitest with `happy-dom` and
  `@testing-library/svelte` for each component: rendering, props, keyboard
  behaviour, focus, ARIA, and the Elyra stores stubbed through `fetch` as
  the runtime's tests do.
- **A browser check per group** in the smoke harness, against a scaffolded
  app in headless Chrome, as the assistant panel has.
- **A gallery page in the example app**, every component in both themes,
  for a person to look at before a release.

## Alternatives considered

- **A native toolkit (FerroUI, an Avalonia port; or egui, Slint).** It
  draws its own pixels and has real desktop controls. But it would replace
  the WebView and Svelte, and with them everything built on them: the
  runtime's built-ins, `make:resource`'s views, the assistant panel, and
  the web skills a team already has. Two UI worlds can't share a component.
- **Build on an existing Svelte library** (shadcn-svelte, Bits UI, Skeleton,
  Flowbite). They're faster to start with, but they know nothing of Elyra's
  stores. They bring their own dependencies (Tailwind, a headless layer) and
  update on their own schedule. A desktop app's keyboard and focus details
  are what we'd end up overriding.
- **Copy-in only** (shadcn's model: no package, every component in the
  app). Each app owns everything, and no fix reaches anyone. Flux, the
  model this RFC follows, ships a package and lets you publish a component
  when you need to.
- **Package only.** An app that needs one change waits for a release or
  forks the package.
- **Put the components in `@elyra/runtime`.** It would make the runtime
  depend on Svelte, and an app without Svelte would carry it.
- **Tailwind classes instead of components.** That isn't what's missing:
  the behaviour is (keyboard, focus, the Elyra stores), not the styling.

## Implementation plan

1. **Tokens.** `ui/tokens.css`, dark and light. The scaffold's `app.css`
   imports it, with the old names as aliases, and the runtime's built-ins
   read the new tokens. The package skeleton, build, tests and release
   asset. Visible on its own: one look for the whole app.
2. **Actions, forms and overlays.** `Button`, `ButtonGroup`, `Dropdown`,
   `Form`, `Field`, `Input`, `Textarea`, `Select`, `Checkbox`, `Switch`,
   `Radio`, `Modal`, `Drawer`, `Tooltip`, `Popover`, with `can` and
   `confirm`. Tests in a DOM.
3. **Data and layout.** `Table` over `Live<Page<T>>`, `Pagination`, `Badge`,
   `Empty`, `AppShell`, `Sidebar`, `NavItem`, `Toolbar`, `StatusBar`,
   `Tabs`, `Card`, `Callout`, `Spinner`, `Skeleton`. The gallery page.
4. **`rata make:ui`**, and `rata new` adding `@elyra/ui`.
5. **`make:resource --view` on `@elyra/ui`.** The smoke harness follows.
6. **Docs** (`docs/ui.md`, one section per component, with the Elyra-aware
   props first), the example app on `@elyra/ui`, and the guidelines that
   `rata ai:install` writes for coding agents.

Each step is a PR; step 1 is useful on its own.

## As built

Step 1:

- **The token set grew by what the built-ins needed:**
  - `--elyra-inset`, for inputs and hovered rows (the scaffold's `--bg-3`);
  - `--elyra-on-accent`, for text on an accent fill;
  - `--elyra-overlay` and `--elyra-shadow`, so dialogs suit a light theme;
  - `--elyra-radius-lg` and `--elyra-font-size`.
- **The aliases live in `tokens.css`,** after the tokens. `--surface` and
  `--muted`, the names the runtime read, are aliases too.
- **The runtime reads `var(--elyra-x, var(<old name>, <default>))`,** so an
  app that never imports the tokens looks as it did. A test keeps every
  colour in the built-ins on a token.
- **`@elyra/ui` ships sources** (`src/`), with no build step, so a checkout
  links it with `file:` as is.

Step 2:

- **`Radio` is `RadioGroup`:** it takes the choices as `options`, with native
  radios inside.
- **`can-mode` is `canMode`:** a Svelte prop can't have a hyphen.
- **`confirmLabel`** names the confirm dialog's button.
- **`@elyra/runtime` is a peer dependency** (`>=0.20.0`): the components use
  its `can`, `confirm`, `href` and `validationErrors`.
- **The gallery came early**, as `ui/gallery/` (`npm run gallery`), for the
  design pass on the first components; it is also the page step 3 asked for.
  The reference replaced it before the release: a page per component in
  `docs/ui/`, served with its examples running by `npm run docs`.
- **Overlays sit in the component's place in the page,** not portalled to
  `<body>`. A popup opens below its button, without collision handling yet.

Step 3:

- **`Table` takes `rows` too:** a plain array is searched, sorted and paged
  in the table, for lists that aren't a command's page.
- **`StatusBar` is quiet when all is well:** online, nothing waiting and
  nothing running show nothing. Only the assistant's button stays.
- **The scaffold dedupes `svelte` and `@elyra/runtime`** in its Vite config.
  A linked `@elyra/ui` (`rata new --elyra`) would otherwise resolve them from
  the checkout, and two runtimes keep two sets of stores.
- **`Card`'s heading row is a `div`,** so the window's toolbar is the only
  banner.

Step 4:

- **The copy comes from `app/node_modules/@elyra/ui`,** the version the app
  has installed, not one built into rata. A copy then matches the package
  the rest of the app uses.
- **A copy imports the package's internals,** which `@elyra/ui` exports as
  `@elyra/ui/internal/*`, instead of copying them too. A copied `Field` and
  the package's `Form` then share one form context; two copies of the
  context's key would keep them apart.
- **`rata new` adding `@elyra/ui` came in step 1.**

Step 5:

- **The table's columns are the database's,** what the index command sorts
  by; a cell shows the field. The first one links to the record.
- **`Table` and `Pagination` take `labels`,** so a generated view's words
  (search, the total, Previous, Next) stay `$t` keys in an app with
  translations.
- **`FileField` stays the app's own** (`FileField.svelte`, written once):
  `@elyra/ui` has no file field yet. *(0.23: `@elyra/ui` has a
  [`FileField`](../ui/file-field.md) now, and `make:resource` uses it.)*
- **`make:resource --view` stops without `@elyra/ui`** in `app/package.json`,
  saying what to add, as it does for `Cargo.toml`.

Step 6:

- **The agents' skill is `elyra-ui`,** written by `rata ai:install` with the
  others, and the guidelines point at it.
- **The example app's todos are on `@elyra/ui`,** and its own styles read
  the tokens.
- **A gap the example showed:** without the `backend` feature,
  `/__auth/abilities` didn't exist, so the `can` store never loaded and `can`
  hid every control. It answers `["*"]` now, and CI tests the shell without
  features.

## Open questions

1. **Distribution:** a package with `rata make:ui` to copy a component
   (recommended), the package only, or copy-in only?
2. **Where it lives:** a new package `@elyra/ui` with Svelte as a peer
   (recommended), or inside `@elyra/runtime`?
3. **The token names:** a new `--elyra-*` set with today's names as aliases
   for one release (recommended), or keep today's names (`--panel`,
   `--text-dim`) and add only what's missing?
4. **The first set:** the list above (recommended), smaller (only what
   `make:resource` uses), or larger (`DataGrid` and `DatePicker` now)?
5. **`make:resource --view`:** generate from `@elyra/ui` from step 5 on
   (recommended), or behind a flag (`--ui`) for a release?
6. **Icons:** a small built-in set as inline SVG for what the components need
   (chevrons, close, check, sort; recommended), or a dependency such as Lucide?
7. **`can` on a control the user can't use:** hide by default (recommended,
   as RFC 0010's generated views do), or disable by default?
8. **The look:** Elyra's own on the current palette (recommended), with a
   short design pass on the first components before step 2 is merged?

## Decisions

Settled on 2026-10-31, all as recommended:

1. **A package, and `rata make:ui`** to copy a component into the app.
2. **A new package, `@elyra/ui`,** with Svelte as a peer dependency;
   `@elyra/runtime` stays framework-free.
3. **New `--elyra-*` tokens,** with today's names (`--panel`, `--text-dim`,
   `--surface`, `--muted`, …) as aliases for one release.
4. **The first set as listed;** `DataGrid`, `Combobox`, `DatePicker`,
   `TreeView` and `SplitPane` later.
5. **`make:resource --view` generates from `@elyra/ui`** from step 5 on,
   without a flag.
6. **A small built-in icon set,** inline SVG, for what the components need.
7. **`can` hides by default;** `can-mode="disable"` shows the control
   disabled.
8. **Elyra's own look** on the current palette, with a short design pass on
   the first components before step 2 is merged.
