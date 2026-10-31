# Design tokens

One set of CSS variables for the whole app: the components, your own
styles, and the runtime's dialogs, toasts, ⌘K palette and assistant panel.
Change a token, and all of them change with it.

<!-- example: tokens/swatches -->
```svelte
<script>
  // The tokens' values, read from the page: switch the theme to see the other set.
  const NAMES = [
    "bg", "surface", "raised", "inset", "text", "muted", "border", "focus",
    "accent", "accent-soft", "on-accent", "danger", "success", "warning",
  ];
  let values = $state({});

  $effect(() => {
    const read = () => {
      const style = getComputedStyle(document.documentElement);
      values = Object.fromEntries(NAMES.map((n) => [n, style.getPropertyValue(`--elyra-${n}`).trim()]));
    };
    read();
    const watch = new MutationObserver(read);
    watch.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => watch.disconnect();
  });
</script>

<div class="swatches">
  {#each NAMES as name (name)}
    <div class="swatch">
      <span class="chip" style:background="var(--elyra-{name})"></span>
      <code>--elyra-{name}</code>
      <small>{values[name]}</small>
    </div>
  {/each}
</div>

<style>
  .swatches { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 10px; width: 100%; }
  .swatch { display: grid; grid-template-columns: auto 1fr; column-gap: 10px; align-items: center; }
  .chip {
    grid-row: span 2; width: 34px; height: 34px; border-radius: var(--elyra-radius);
    box-shadow: inset 0 0 0 1px var(--elyra-border);
  }
  code { font: 12px var(--elyra-font-mono); }
  small { font: 11px var(--elyra-font-mono); color: var(--elyra-muted); }
</style>
```

The tokens are in `@elyra/ui/tokens.css`. Import it in `main.js`, before the
app's own CSS:

```js
import "@elyra/ui/tokens.css";
import "./app.css";
```

## Themes

The tokens are dark by default, on the Tokyo Night palette.
`data-theme="light"` on the `<html>` element switches to the light set. A new
app's `src/lib/theme.js` sets it: `auto` follows the system, and `light` or
`dark` is the user's choice, kept between launches.

To change a token, set it in the app's CSS. Set it in each theme you change:

```css
:root { --elyra-accent: #2ac3de; }
:root[data-theme="light"] { --elyra-accent: #0f7f93; }
```

## Your own styles

Build the app's own components on the tokens, and they follow the theme.
Don't write colours of your own.

<!-- example: tokens/own-styles -->
```svelte
<!-- An app's own component, on the tokens: it follows the theme with the rest. -->
<div class="invoice">
  <span class="number">INV-2041</span>
  <span class="amount">1 860 €</span>
  <span class="due">Overdue since 14 October</span>
</div>

<style>
  .invoice {
    display: grid; grid-template-columns: 1fr auto; gap: 2px 16px;
    width: 280px; padding: calc(var(--elyra-space) * 2);
    background: var(--elyra-raised);
    border: 1px solid var(--elyra-border);
    border-radius: var(--elyra-radius-lg);
    font-family: var(--elyra-font);
  }
  .number { font-family: var(--elyra-font-mono); color: var(--elyra-muted); }
  .amount { font-weight: 600; }
  .due { grid-column: 1 / -1; color: var(--elyra-danger); }
</style>
```

## Surfaces

From the window down to what sits on it.

| Token | For |
|---|---|
| `--elyra-bg` | The window, and the page. |
| `--elyra-surface` | Bars and sidebars: the toolbar, the status bar, the navigation. |
| `--elyra-raised` | What sits on the page: cards, dialogs, menus, popovers. |
| `--elyra-inset` | What's set into it: inputs, a hovered row, code. |

## Text and lines

| Token | For |
|---|---|
| `--elyra-text` | Text. |
| `--elyra-muted` | Secondary text: descriptions, help, counts. |
| `--elyra-border` | Borders and separators. |
| `--elyra-focus` | The focus ring. |

## Accent and states

| Token | For |
|---|---|
| `--elyra-accent` | The app's colour: the primary button, the current page, links. |
| `--elyra-accent-soft` | A quiet fill of the accent: the current nav item, an accent badge. |
| `--elyra-on-accent` | Text on an accent fill. |
| `--elyra-danger` | What destroys, and what failed: errors, a danger button. |
| `--elyra-success` | What went well. |
| `--elyra-warning` | What needs a look. |

## Depth, shape and type

| Token | For |
|---|---|
| `--elyra-shadow` | Under what floats: dialogs, menus, popovers. |
| `--elyra-overlay` | Behind a modal. |
| `--elyra-radius` | Controls: buttons, inputs, menu items (8px). |
| `--elyra-radius-lg` | Panels: cards, dialogs (14px). |
| `--elyra-space` | The spacing unit (8px). Use multiples of it. |
| `--elyra-font` | The system's UI font. |
| `--elyra-font-mono` | Code and numbers that line up. |
| `--elyra-font-size` | The body size (14px). |

## The old names

Apps and generated views made before 0.21 used `--bg`, `--bg-2`, `--bg-3`,
`--panel`, `--surface`, `--border`, `--text`, `--text-dim`, `--muted`,
`--accent`, `--accent-2`, `--green`, `--red`, `--amber`, `--font-ui` and
`--font-mono`. They're kept as aliases of the tokens for one release. New
code uses the `--elyra-*` names.
