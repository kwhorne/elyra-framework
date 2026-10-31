<!--
  Views of one thing, one at a time: `role="tablist"`. ←/→, Home and End
  move between tabs (and show them). `tabs` are `{ id, label, can, disabled }`,
  `bind:value` the shown one's id, and `panel(id)` the snippet that draws it.
-->
<script>
  import { can as canStore } from "@elyra/runtime";

  let { tabs = [], value = $bindable(undefined), label = undefined, class: className = "", panel } = $props();

  const own = $props.id();
  const shown = $derived(tabs.filter((t) => !t.can || $canStore(t.can)));
  const current = $derived(shown.some((t) => t.id === value) ? value : shown.find((t) => !t.disabled)?.id);
  let list = $state();

  function select(tab, focus = false) {
    if (!tab || tab.disabled) return;
    value = tab.id;
    if (focus) list?.querySelector(`#${CSS.escape(`${own}-tab-${tab.id}`)}`)?.focus();
  }

  function onKey(event) {
    const usable = shown.filter((t) => !t.disabled);
    const at = usable.findIndex((t) => t.id === current);
    const to = {
      ArrowRight: usable[(at + 1) % usable.length],
      ArrowLeft: usable[(at - 1 + usable.length) % usable.length],
      Home: usable[0],
      End: usable[usable.length - 1],
    }[event.key];
    if (to) {
      event.preventDefault();
      select(to, true);
    }
  }
</script>

<div class="elyra-tabs {className}">
  <div bind:this={list} role="tablist" aria-label={label} tabindex="-1" onkeydown={onKey}>
    {#each shown as tab (tab.id)}
      <button
        type="button"
        role="tab"
        id="{own}-tab-{tab.id}"
        aria-selected={tab.id === current}
        aria-controls="{own}-panel"
        tabindex={tab.id === current ? 0 : -1}
        disabled={tab.disabled}
        onclick={() => select(tab)}
      >
        {tab.label}
      </button>
    {/each}
  </div>
  <div id="{own}-panel" role="tabpanel" aria-labelledby="{own}-tab-{current}" tabindex="0" class="panel">
    {#if current !== undefined}{@render panel?.(current)}{/if}
  </div>
</div>

<style>
  .elyra-tabs { display: grid; gap: 14px; font: 13px var(--elyra-font); color: var(--elyra-text); min-width: 0; }
  [role="tablist"] { display: flex; gap: 2px; border-bottom: 1px solid var(--elyra-border); outline: none; overflow-x: auto; }
  [role="tab"] {
    position: relative; height: 34px; padding: 0 12px; border: 0; background: transparent;
    color: var(--elyra-muted); font: 500 13px var(--elyra-font); cursor: pointer; white-space: nowrap;
  }
  [role="tab"]:hover:not(:disabled) { color: var(--elyra-text); }
  [role="tab"][aria-selected="true"] { color: var(--elyra-text); }
  [role="tab"][aria-selected="true"]::after {
    content: ""; position: absolute; left: 8px; right: 8px; bottom: -1px; height: 2px; border-radius: 2px; background: var(--elyra-accent);
  }
  [role="tab"]:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: -2px; border-radius: 4px; }
  [role="tab"]:disabled { opacity: .45; cursor: default; }
  .panel { outline: none; }
  .panel:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; border-radius: 4px; }
</style>
