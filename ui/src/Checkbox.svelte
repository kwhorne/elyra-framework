<!--
  A checkbox with its label beside it: the native input, drawn to match.
  `indeterminate` shows it as partly checked (some of a list, not all).
-->
<script>
  import { getContext } from "svelte";
  import Icon from "./internal/Icon.svelte";
  import { FIELD } from "./internal/context.js";

  let { checked = $bindable(false), indeterminate = $bindable(false), disabled = false, class: className = "", children, ...rest } = $props();
  const field = getContext(FIELD);
</script>

<label class="elyra-checkbox {className}" class:disabled>
  <input
    type="checkbox"
    bind:checked
    bind:indeterminate
    {disabled}
    id={field?.id}
    name={field?.name}
    aria-invalid={field?.invalid || undefined}
    aria-describedby={field?.describedBy}
    {...rest}
  />
  <span class="box" aria-hidden="true"><Icon name={indeterminate ? "minus" : "check"} size={12} /></span>
  {#if children}<span class="text">{@render children()}</span>{/if}
</label>

<style>
  .elyra-checkbox { display: inline-flex; align-items: center; gap: 8px; cursor: pointer; color: var(--elyra-text); font: 13px var(--elyra-font); }
  .elyra-checkbox.disabled { opacity: .55; cursor: default; }
  input { position: absolute; opacity: 0; width: 1px; height: 1px; margin: 0; }
  .box {
    display: inline-grid; place-items: center; width: 16px; height: 16px; flex: none;
    border: 1px solid var(--elyra-border); border-radius: 4px; background: var(--elyra-inset);
    color: transparent; transition: background-color .12s, border-color .12s;
  }
  input:checked + .box, input:indeterminate + .box { background: var(--elyra-accent); border-color: var(--elyra-accent); color: var(--elyra-on-accent); }
  input:focus-visible + .box { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  input[aria-invalid="true"] + .box { border-color: var(--elyra-danger); }
</style>
