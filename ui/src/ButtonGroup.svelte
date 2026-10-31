<!-- Buttons that belong together, joined. -->
<script>
  let { label = undefined, class: className = "", children, ...rest } = $props();
</script>

<div role="group" aria-label={label} class="elyra-button-group {className}" {...rest}>
  {@render children?.()}
</div>

<style>
  .elyra-button-group { display: inline-flex; }
  .elyra-button-group :global(.elyra-button) { border-radius: 0; margin-left: -1px; }
  .elyra-button-group :global(.elyra-button:first-child) {
    margin-left: 0;
    border-radius: var(--elyra-radius) 0 0 var(--elyra-radius);
  }
  .elyra-button-group :global(.elyra-button:last-child) { border-radius: 0 var(--elyra-radius) var(--elyra-radius) 0; }
  .elyra-button-group :global(.elyra-button:focus-visible) { position: relative; z-index: 1; }
</style>
