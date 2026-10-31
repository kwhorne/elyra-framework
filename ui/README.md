# @elyra/ui

Desktop components for [Elyra Framework](https://github.com/kwhorne/elyra-framework)
apps, in Svelte 5 ([RFC 0012](https://github.com/kwhorne/elyra-framework/blob/main/docs/proposals/0012-elyra-ui.md)),
and the design tokens the app, these components and `@elyra/runtime`'s
built-ins share.

```js
// app/src/main.js
import "@elyra/ui/tokens.css";
import "./app.css";
```

The tokens are `--elyra-*` CSS variables: dark by default, light under
`[data-theme="light"]`. Override any of them in the app's CSS. The names used
before 0.21 (`--bg`, `--panel`, `--text-dim`, …) are kept as aliases for one
release.

```svelte
<script>
  import { Button, Field, Form, Input } from "@elyra/ui";
</script>

<Form onsubmit={save}>
  <Field name="email" label="Email"><Input type="email" bind:value={email} /></Field>
  <Button type="submit" variant="primary">Save</Button>
</Form>
```

36 components: buttons and menus, forms and their fields (a searchable combobox and a file field among them), dialogs, drawers,
popovers and tooltips, a table over a live query, the window's shell and split panes,
navigation and status bar, the outbox's drawer, tabs, cards and callouts. They're keyboard-first,
with ARIA, `can` for the user's abilities, and Laravel's validation errors
per field. Each one has a page in
[the reference](https://github.com/kwhorne/elyra-framework/blob/main/docs/ui.md).
`npm run docs` serves it with every example running, in both themes.

MIT licensed.
