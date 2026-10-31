<!--
  A panel next to a button, for more than a menu: a filter, a few fields.
  Non-modal (`role="dialog"`): focus moves in, Esc or a click outside
  closes it and focus goes back to the button.
-->
<script>
  import { tick } from "svelte";
  import Icon from "./internal/Icon.svelte";
  import { focusables, onOutside } from "./internal/focus.js";

  let {
    open = $bindable(false),
    label = undefined,
    trigger = undefined,
    variant = "default",
    align = "start",
    class: className = "",
    children,
  } = $props();

  const own = $props.id();
  let root = $state();
  let button = $state();
  let panel = $state();

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) button?.focus();
  }

  $effect(() => {
    if (!open) return;
    tick().then(() => (focusables(panel)[0] ?? panel)?.focus());
    return onOutside(() => [root], () => close(false));
  });
</script>

<div class="elyra-popover {className}" bind:this={root}>
  <button
    bind:this={button}
    type="button"
    class="elyra-button {variant}"
    aria-haspopup="dialog"
    aria-expanded={open}
    aria-controls="{own}-panel"
    onclick={() => (open ? close() : (open = true))}
  >
    {#if trigger}{@render trigger()}{:else}{label}{/if}
    <Icon name="chevron-down" size={12} />
  </button>
  {#if open}
    <div
      bind:this={panel}
      id="{own}-panel"
      role="dialog"
      aria-label={label}
      tabindex="-1"
      class="panel {align}"
      onkeydown={(event) => event.key === "Escape" && (event.stopPropagation(), close())}
    >
      {@render children?.()}
    </div>
  {/if}
</div>

<style>
  .elyra-popover { position: relative; display: inline-block; font: 13px var(--elyra-font); }
  .elyra-button {
    display: inline-flex; align-items: center; gap: 6px; height: 32px; padding: 0 10px 0 12px;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    background: var(--elyra-inset); color: var(--elyra-text); font: 500 13px var(--elyra-font); cursor: pointer;
  }
  .elyra-button.ghost { background: transparent; border-color: transparent; color: var(--elyra-muted); }
  .elyra-button:hover { border-color: var(--elyra-accent); }
  .elyra-button:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  .panel {
    position: absolute; z-index: 60; top: calc(100% + 6px); left: 0; min-width: 240px; padding: 12px;
    background: var(--elyra-raised); color: var(--elyra-text); border: 1px solid var(--elyra-border);
    border-radius: var(--elyra-radius-lg); box-shadow: var(--elyra-shadow); outline: none;
  }
  .panel.end { left: auto; right: 0; }
</style>
