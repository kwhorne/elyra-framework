<!-- A line between groups: `role="separator"`. -->
<script>
  let { orientation = "horizontal", class: className = "" } = $props();
</script>

<div role="separator" aria-orientation={orientation} class="elyra-separator {orientation} {className}"></div>

<style>
  .elyra-separator { background: var(--elyra-border); flex: none; }
  .horizontal { height: 1px; width: 100%; }
  .vertical { width: 1px; align-self: stretch; }
</style>
