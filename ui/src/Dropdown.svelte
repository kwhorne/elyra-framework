<!--
  A button that opens a menu of `<MenuItem>`s: `role="menu"`, with ↓/↑,
  Home/End, Enter and Esc. `label` is the button's text, or `trigger` a
  snippet for its content; `align="end"` lines the menu up on the right.
-->
<script>
  import { setContext, tick } from "svelte";
  import Icon from "./internal/Icon.svelte";
  import { MENU } from "./internal/context.js";
  import { onOutside } from "./internal/focus.js";

  let {
    label = undefined,
    trigger = undefined,
    variant = "default",
    size = "md",
    align = "start",
    disabled = false,
    class: className = "",
    children,
  } = $props();

  const own = $props.id();
  let open = $state(false);
  let root = $state();
  let button = $state();
  let menu = $state();

  const items = () => [...(menu?.querySelectorAll('[role="menuitem"]:not([disabled])') ?? [])];

  async function show(focus = "first") {
    if (disabled) return;
    open = true;
    await tick();
    const list = items();
    (focus === "last" ? list[list.length - 1] : list[0])?.focus();
  }

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) button?.focus();
  }

  setContext(MENU, { close });

  $effect(() => {
    if (open) return onOutside(() => [root], () => close(false));
  });

  function onButtonKey(event) {
    if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      show("first");
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      show("last");
    }
  }

  function onMenuKey(event) {
    const list = items();
    const at = list.indexOf(document.activeElement);
    const go = (i) => list[(i + list.length) % list.length]?.focus();
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        go(at + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        go(at - 1);
        break;
      case "Home":
        event.preventDefault();
        go(0);
        break;
      case "End":
        event.preventDefault();
        go(list.length - 1);
        break;
      case "Escape":
        event.preventDefault();
        close();
        break;
      case "Tab":
        close(false);
        break;
    }
  }
</script>

<div class="elyra-dropdown {className}" bind:this={root}>
  <button
    bind:this={button}
    type="button"
    id="{own}-button"
    class="elyra-button {variant} {size}"
    aria-haspopup="menu"
    aria-expanded={open}
    aria-controls="{own}-menu"
    {disabled}
    onclick={() => (open ? close() : show())}
    onkeydown={onButtonKey}
  >
    {#if trigger}{@render trigger()}{:else}{label}{/if}
    <Icon name="chevron-down" size={12} />
  </button>
  {#if open}
    <div
      bind:this={menu}
      id="{own}-menu"
      role="menu"
      tabindex="-1"
      aria-labelledby="{own}-button"
      class="menu {align}"
      onkeydown={onMenuKey}
    >
      {@render children?.()}
    </div>
  {/if}
</div>

<style>
  .elyra-dropdown { position: relative; display: inline-block; font: 13px var(--elyra-font); }
  .elyra-button {
    display: inline-flex; align-items: center; gap: 6px; height: 32px; padding: 0 10px 0 12px;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    background: var(--elyra-inset); color: var(--elyra-text); font: 500 13px var(--elyra-font); cursor: pointer;
  }
  .elyra-button.sm { height: 26px; font-size: 12px; padding: 0 8px; }
  .elyra-button.ghost { background: transparent; border-color: transparent; color: var(--elyra-muted); }
  .elyra-button.primary { background: var(--elyra-accent); border-color: var(--elyra-accent); color: var(--elyra-on-accent); }
  .elyra-button:hover:not(:disabled) { border-color: var(--elyra-accent); }
  .elyra-button:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  .elyra-button:disabled { opacity: .5; cursor: default; }
  .menu {
    position: absolute; z-index: 60; top: calc(100% + 4px); left: 0; min-width: 180px; padding: 4px;
    background: var(--elyra-raised); border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    box-shadow: var(--elyra-shadow); outline: none;
  }
  .menu.end { left: auto; right: 0; }
</style>
