<!-- The app's navigation down the side: `<NavItem>`s, and a `footer` snippet. -->
<script>
  let { label = "Main", class: className = "", header = undefined, footer = undefined, children } = $props();
</script>

<aside class="elyra-sidebar {className}">
  {#if header}<div class="header">{@render header()}</div>{/if}
  <nav aria-label={label}>{@render children?.()}</nav>
  {#if footer}<div class="footer">{@render footer()}</div>{/if}
</aside>

<style>
  .elyra-sidebar {
    display: flex; flex-direction: column; gap: 12px; width: 220px; flex: none; box-sizing: border-box; padding: 12px 10px;
    background: var(--elyra-surface); border-right: 1px solid var(--elyra-border); overflow-y: auto;
  }
  nav { display: grid; gap: 2px; }
  .header { padding: 4px 6px; }
  .footer { margin-top: auto; }
</style>
