<!-- A short label: a status, a count, a tag. -->
<script>
  let { variant = "neutral", class: className = "", children, ...rest } = $props();
</script>

<span class="elyra-badge {variant} {className}" {...rest}>{@render children?.()}</span>

<style>
  .elyra-badge {
    display: inline-flex; align-items: center; gap: 4px; height: 20px; padding: 0 7px; box-sizing: border-box;
    border-radius: 999px; font: 600 11px/1 var(--elyra-font); white-space: nowrap;
    background: var(--elyra-inset); color: var(--elyra-muted); border: 1px solid var(--elyra-border);
  }
  .accent { background: var(--elyra-accent-soft); color: var(--elyra-accent); border-color: transparent; }
  .success, .warning, .danger { border-color: transparent; }
  .success { background: color-mix(in srgb, var(--elyra-success) 16%, transparent); color: var(--elyra-success); }
  .warning { background: color-mix(in srgb, var(--elyra-warning) 18%, transparent); color: var(--elyra-warning); }
  .danger { background: color-mix(in srgb, var(--elyra-danger) 14%, transparent); color: var(--elyra-danger); }
</style>
