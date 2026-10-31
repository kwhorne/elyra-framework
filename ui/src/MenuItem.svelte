<!--
  One choice in a `<Dropdown>`. Picking it runs `onclick` and closes the
  menu. `can` hides it from a user without the ability (`canMode="disable"`
  shows it disabled); `danger` colours it.
-->
<script>
  import { getContext } from "svelte";
  import { can as canStore } from "@elyra/runtime";
  import { MENU } from "./internal/context.js";

  let {
    onclick = undefined,
    disabled = false,
    danger = false,
    can = undefined,
    canMode = "hide",
    class: className = "",
    children,
    ...rest
  } = $props();

  const menu = getContext(MENU);
  const allowed = $derived(can ? $canStore(can) : true);

  async function choose(event) {
    menu?.close();
    await onclick?.(event);
  }
</script>

{#if allowed || canMode === "disable"}
  <button
    type="button"
    role="menuitem"
    tabindex="-1"
    disabled={disabled || !allowed}
    class="elyra-menu-item {className}"
    class:danger
    onclick={choose}
    {...rest}
  >
    {@render children?.()}
  </button>
{/if}

<style>
  .elyra-menu-item {
    display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 8px; box-sizing: border-box;
    border: 0; border-radius: calc(var(--elyra-radius) - 2px); background: transparent;
    color: var(--elyra-text); font: 13px var(--elyra-font); text-align: left; cursor: pointer;
  }
  .elyra-menu-item:hover:not(:disabled), .elyra-menu-item:focus { outline: none; background: var(--elyra-accent-soft); }
  .elyra-menu-item.danger { color: var(--elyra-danger); }
  .elyra-menu-item:disabled { opacity: .45; cursor: default; }
</style>
