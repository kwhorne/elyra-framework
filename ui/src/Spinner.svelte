<!-- Something is on its way. `label` is what a screen reader hears. -->
<script>
  let { size = "md", label = "Loading", class: className = "" } = $props();
</script>

<span role="status" aria-label={label} class="elyra-spinner {size} {className}"></span>

<style>
  .elyra-spinner {
    display: inline-block; width: 16px; height: 16px; box-sizing: border-box; border-radius: 50%;
    border: 2px solid var(--elyra-border); border-top-color: var(--elyra-accent);
    animation: elyra-spin .7s linear infinite; vertical-align: middle;
  }
  .sm { width: 12px; height: 12px; }
  .lg { width: 24px; height: 24px; border-width: 3px; }
  @keyframes elyra-spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .elyra-spinner { animation-duration: 2s; } }
</style>
