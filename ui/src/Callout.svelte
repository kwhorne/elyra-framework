<!--
  A message in the page: `info`, `success`, `warning` or `danger`. A `danger`
  or `warning` one is announced (`role="alert"`). `dismissible` adds a close
  button; `ondismiss` is told.
-->
<script>
  import Icon from "./internal/Icon.svelte";

  let { variant = "info", title = undefined, dismissible = false, ondismiss = undefined, class: className = "", children } = $props();
  let shown = $state(true);
  const urgent = $derived(variant === "danger" || variant === "warning");
</script>

{#if shown}
  <div class="elyra-callout {variant} {className}" role={urgent ? "alert" : "status"}>
    <div class="text">
      {#if title}<strong>{title}</strong>{/if}
      {#if children}<div>{@render children()}</div>{/if}
    </div>
    {#if dismissible}
      <button type="button" class="close" aria-label="Dismiss" onclick={() => ((shown = false), ondismiss?.())}>
        <Icon name="close" size={12} />
      </button>
    {/if}
  </div>
{/if}

<style>
  .elyra-callout {
    --tone: var(--elyra-accent);
    display: flex; gap: 10px; align-items: flex-start; padding: 10px 12px; border-radius: var(--elyra-radius);
    background: color-mix(in srgb, var(--tone) 10%, transparent);
    border: 1px solid color-mix(in srgb, var(--tone) 35%, transparent);
    color: var(--elyra-text); font: 13px/1.45 var(--elyra-font);
  }
  .success { --tone: var(--elyra-success); }
  .warning { --tone: var(--elyra-warning); }
  .danger { --tone: var(--elyra-danger); }
  .text { flex: 1; min-width: 0; display: grid; gap: 2px; }
  strong { color: var(--tone); font-weight: 600; }
  .close {
    display: inline-grid; place-items: center; width: 22px; height: 22px; flex: none;
    border: 0; border-radius: 6px; background: transparent; color: var(--elyra-muted); cursor: pointer;
  }
  .close:hover { color: var(--elyra-text); background: color-mix(in srgb, var(--tone) 14%, transparent); }
  .close:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
</style>
