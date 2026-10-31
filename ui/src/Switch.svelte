<!-- On or off, now: `role="switch"`. Space or Enter toggles it. -->
<script>
  import { getContext } from "svelte";
  import { FIELD } from "./internal/context.js";

  let { checked = $bindable(false), disabled = false, onchange = undefined, class: className = "", children, ...rest } = $props();
  const field = getContext(FIELD);
  const own = $props.id();
  const id = $derived(field?.id ?? own);

  function toggle() {
    if (disabled) return;
    checked = !checked;
    onchange?.(checked);
  }
</script>

<span class="elyra-switch-wrap {className}">
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    {disabled}
    {id}
    aria-describedby={field?.describedBy}
    class="elyra-switch"
    onclick={toggle}
    {...rest}
  >
    <span class="thumb" aria-hidden="true"></span>
  </button>
  {#if children}<label for={id} class="text">{@render children()}</label>{/if}
</span>

<style>
  .elyra-switch-wrap { display: inline-flex; align-items: center; gap: 8px; font: 13px var(--elyra-font); color: var(--elyra-text); }
  .elyra-switch {
    position: relative; width: 32px; height: 18px; padding: 0; flex: none;
    border: 1px solid var(--elyra-border); border-radius: 999px; background: var(--elyra-inset);
    cursor: pointer; transition: background-color .15s, border-color .15s;
  }
  .elyra-switch[aria-checked="true"] { background: var(--elyra-accent); border-color: var(--elyra-accent); }
  .elyra-switch:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  .elyra-switch:disabled { opacity: .5; cursor: default; }
  .thumb {
    position: absolute; top: 2px; left: 2px; width: 12px; height: 12px; border-radius: 50%;
    background: var(--elyra-muted); transition: transform .15s, background-color .15s;
  }
  .elyra-switch[aria-checked="true"] .thumb { transform: translateX(14px); background: var(--elyra-on-accent); }
</style>
