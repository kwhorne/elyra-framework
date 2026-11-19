# RFC 0037 — Theme Studio: the app's own look, checked as it's made

**Status:** accepted (2026-11-19) · **Target:** 0.51.0 · builds on [RFC 0012](0012-elyra-ui.md), [RFC 0014](0014-elyra-inspector.md) and [RFC 0034](0034-the-state-matrix.md) · see [Decisions](#decisions)

## Summary

Every Elyra app is drawn from the same tokens in `@elyra/ui/tokens.css`:
about twenty colours, a radius, a spacing, a font. An app that changes
none of them looks like every other Elyra app, and one that changes them
by hand finds out about a contrast it broke when a user can't read a
label. Theme Studio is a panel in the inspector, in a debug build, for
changing them while seeing the app:

- **Drag a colour, the radius or the spacing**, and every window of the
  app changes at once: the frames of the matrix (RFC 0034) too, so dark,
  light and more contrast are seen side by side.
- **Every pair that has to be readable is checked as you go**: text and
  muted text on each surface, the accent's text on the accent, borders
  and the focus ring. A pair that falls below its ratio says so, with the
  ratio.
- **Copy the result as CSS** for `app/src/app.css`, only the tokens that
  changed, for dark and for light.

The same check runs on the framework's own tokens in its tests, so what
`tokens.css` promises ("secondary text at 7:1 or more") is held.

## Motivation

- **The tokens are the app's look, and only a few people change them.**
  `rata new` writes an `app.css` that says "override any token here", and
  then the developer has to know which of the twenty does what, and see
  each in dark, light and more contrast.
- **Contrast is easy to break and hard to see, the framework's own
  included.** Computed for this RFC, `--elyra-muted` in the dark theme is
  4.40:1 on `--elyra-bg` and 3.93:1 on `--elyra-inset`, and in light 4.25:1
  on `--elyra-inset`: under the 4.5:1 WCAG asks of text. It looks fine on
  a good screen. Every other text pair passes, from 4.74:1 (the accent's
  text in light) to 13.55:1.
- **`tokens.css` makes promises nothing checks.** Its more-contrast block
  says secondary text is at 7:1 or more and borders and focus at 3:1 on
  every surface. They hold today (muted 7.61:1 at least, borders 3.30:1),
  but no test computes a ratio, so the next change could break them.
- **The matrix already shows every look at once.** What's missing is a way
  to change the tokens and watch all of them follow.

## Current state (0.49)

- **`tokens.css`**: dark under `:root`, light under
  `[data-theme="light"]`, more contrast under `prefers-contrast: more` and
  `[data-contrast="more"]`, motion under `prefers-reduced-motion`. The
  tests check that the themes declare the same tokens and that the two
  more-contrast blocks agree. No ratio is computed.
- **`app.css`** in a new app imports `tokens.css` first, and overrides
  after it.
- **The inspector** (RFC 0014) is a drawer in the page, debug only. **The
  matrix** (RFC 0034) opens frames that are real windows.

## Design

### The panel

**Theme** in the inspector's header opens the studio in the drawer:

- **Colours**, grouped as `tokens.css` groups them (surfaces, text,
  border and focus, accent, status), each with a colour picker and its
  hex value, for the theme being edited: **Dark** or **Light**, a switch at
  the top. More contrast is edited under its own switch, since its values
  are separate.
- **Shape and space**: `--elyra-radius`, `--elyra-radius-lg` and
  `--elyra-space` as sliders, and `--elyra-font-size`.
- **Font**: `--elyra-font` from a short list (the system font, and the
  fonts the page has loaded), or typed.
- **Reset** each token, or all.

### Every window follows

A change is sent to the shell (`/__inspector/theme`), which keeps the
draft for this run and sends it on `elyra:theme-draft` to every window.
`@elyra/runtime` writes it into one `<style id="elyra-theme-draft">` at the
end of `<head>`, after the app's CSS, so it wins:

```css
:root { --elyra-accent: #f97316; }
:root[data-theme="light"] { --elyra-accent: #c2410c; }
```

Each window draws it in its own look: a matrix frame in light shows the
light value. A window opened later gets the draft when it starts. Nothing
is written to a file, and quitting the app forgets the draft.

### The checks

For the theme being edited, each pair is computed from the colours the
page actually resolves (`getComputedStyle`), with WCAG's relative
luminance, and listed with its ratio:

| Pair | Needs | More contrast needs |
|---|---|---|
| `--elyra-text` on each surface (`bg`, `surface`, `raised`, `inset`) | 4.5:1 | 7:1 |
| `--elyra-muted` on each surface | 4.5:1 | 7:1 |
| `--elyra-on-accent` on `--elyra-accent` | 4.5:1 | 4.5:1 |
| `--elyra-danger`, `--elyra-success`, `--elyra-warning` on `bg` | 3:1 | 4.5:1 |
| `--elyra-border` on each surface | — (a divider; today 1.15–1.39:1) | 3:1 |
| `--elyra-focus` on each surface | 3:1 | 3:1 |

A pair below its ratio is marked, with the ratio and the colour that
would meet it nearest to the one chosen ("#8a8fb0 would be 4.5:1").
`--elyra-border` is a divider in the default look, and isn't held to a
ratio there; an input's edge is told by its `--elyra-inset` fill as much as
its border. Under more contrast, where `tokens.css` promises 3:1, it is.
Status colours are held to 3:1, since `@elyra/ui` never sets body text in
them; their text on a soft background is checked as text.

### Taking it with you

**Copy as CSS** puts the changed tokens on the clipboard as blocks for
`app/src/app.css`: `:root`, `:root[data-theme="light"]`, and the
more-contrast blocks only when they changed. Only what differs from
`tokens.css` is in it, with a comment saying where it came from and which
checks it passes.

### In the framework's tests

The contrast function lives in `@elyra/ui` (`contrast(a, b)`, exported
for an app's own tests) and the runtime's studio uses the same. A new test
in `tokens.test.js` computes every pair above for dark, light and both
more-contrast blocks, so `tokens.css` can't drift below what it promises.

### Security

Debug only, as the inspector is. The draft lives in the shell's memory
for the run and is a stylesheet of `--elyra-*` values: the shell accepts
only token names it knows and values without `;`, `{`, `}` or `url(`, so
the draft can't carry other CSS into the windows.

### Testing

- `contrast()` against WCAG's examples, and the tokens test above.
- The runtime: the draft lands in one `<style>` after the app's, a new
  window gets it, and a value that isn't a token or carries `}` is
  refused.
- The shell: `/__inspector/theme` keeps, sends and resets the draft; not
  in a release (`inspector_release.rs`).
- By hand: the example app with the matrix open, the accent dragged, the
  three frames following, a failing pair marked, the CSS copied into
  `app.css`.

## Implementation plan

1. `contrast()` in `@elyra/ui`, and the tokens test. `--elyra-muted`
   changes in both themes to reach 4.5:1 on every surface, as close to
   today's colour as it can be.
2. `/__inspector/theme` and `elyra:theme-draft`; the runtime's
   `<style>`.
3. The panel in the inspector: tokens, checks, copy as CSS.
4. Docs: `docs/ui/tokens.md` (a section), `docs/inspector.md`.

## Not in this RFC

- **Writing `app.css` from the app.** The app would edit the developer's
  source; the clipboard is a step more, and leaves the file to the
  developer and Git.
- **A palette from one colour.** Deriving `accent-soft`, `focus` and the
  surfaces from a brand colour is a design question of its own.
- **A check in `rata check`** of an app's own overrides in `app.css`. It
  would need the cascade resolved without a browser; the studio and the
  app's tests with `contrast()` cover it for now.

## Alternatives considered

- **A page in the `@elyra/ui` reference** (`npm run docs`) instead of the
  inspector. It shows the components, not the app, and an app's own
  screens are what the tokens have to work for.
- **Editing in the browser's dev tools.** The tokens are there, but one
  at a time, in one window, with no ratios and nothing to copy back.

## Decisions

1. **In the inspector**, as a panel, as the other developer tools are.
2. **The draft lives in the shell**, so every window and every matrix
   frame follows.
3. **Status colours at 3:1**, and 4.5:1 under more contrast. Light's are
   3.39–4.36:1 today, so its more-contrast block gets darker ones.
4. **Copy, not write.** The CSS goes on the clipboard; the app never edits
   `app.css`.
5. **`--elyra-muted` changes** in 0.51 to reach 4.5:1 on every surface,
   as close to today's colour as it can be, noted under Changed in the
   changelog.
6. **Borders are dividers** in the default look, held to no ratio there,
   and to 3:1 under more contrast.
