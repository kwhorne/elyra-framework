<!-- The bar at the top of a window or a page: a title, then `actions` on the right. -->
<script>
  let { title = undefined, class: className = "", actions = undefined, children } = $props();
</script>

<header class="elyra-toolbar {className}">
  {#if title}<strong class="title">{title}</strong>{/if}
  {@render children?.()}
  {#if actions}<div class="actions">{@render actions()}</div>{/if}
</header>

<style>
  .elyra-toolbar {
    display: flex; align-items: center; gap: 12px; min-height: 44px; padding: 0 14px; box-sizing: border-box; flex: none;
    background: var(--elyra-surface); border-bottom: 1px solid var(--elyra-border);
    color: var(--elyra-text); font: 13px var(--elyra-font);
  }
  .title { font-size: 14px; font-weight: 700; letter-spacing: .01em; }
  .actions { display: flex; align-items: center; gap: 8px; margin-left: auto; }
</style>
