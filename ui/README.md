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

The components arrive in the following releases.

MIT licensed.
