<script>
  import Showcase from "./Showcase.svelte";
</script>

<main class="gallery">
  <header>
    <p class="eyebrow">@elyra/ui · RFC 0012, step 2</p>
    <h1>The first components, in both themes</h1>
    <p class="lede">
      Buttons, a menu, a form with its fields, and the overlays, on the design tokens every
      Elyra app now has. Everything works: open the menu with ↓, pick a country by typing,
      save the form (the email is wrong on purpose), open the modal and press Tab and Esc.
    </p>
  </header>
  <div class="columns">
    <Showcase theme="dark" />
    <Showcase theme="light" />
  </div>
</main>

<style>
  /* The page itself: Elyra's own tokens, following the viewer's theme. */
  :global(:root) {
    --page-bg: #f5f6fa; --page-text: #2a2e3a; --page-muted: #6b7080; --page-accent: #3b6fd6;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root:not([data-theme="light"])) {
      --page-bg: #111118; --page-text: #c0caf5; --page-muted: #787c99; --page-accent: #7aa2f7; color-scheme: dark;
    }
  }
  :global(:root[data-theme="dark"]) {
    --page-bg: #111118; --page-text: #c0caf5; --page-muted: #787c99; --page-accent: #7aa2f7; color-scheme: dark;
  }
  :global(body) { background: var(--page-bg); color: var(--page-text); }

  .gallery {
    max-width: 1180px; margin: 0 auto;
    padding-block: 32px 48px;
    padding-left: max(16px, env(safe-area-inset-left, 0px));
    padding-right: max(16px, env(safe-area-inset-right, 0px));
    font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  }
  header { max-width: 68ch; margin-bottom: 24px; }
  .eyebrow { margin: 0 0 6px; font-size: 12px; letter-spacing: .06em; text-transform: uppercase; color: var(--page-accent); }
  h1 { margin: 0 0 10px; font-size: 26px; line-height: 1.2; text-wrap: balance; }
  .lede { margin: 0; color: var(--page-muted); }
  .columns { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 460px), 1fr)); gap: 20px; }
</style>
