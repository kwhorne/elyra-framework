<!--
  Views of one thing, one at a time: `role="tablist"`. ←/→, Home and End
  move between tabs (and show them). `tabs` are `{ id, label, can, disabled }`,
  `bind:value` the shown one's id, and `panel(id)` the snippet that draws it.
  The line under the shown tab glides to the next one (instantly, for a user
  who asked for less motion).
-->
<script>
  import { can as canStore } from "@elyra/runtime";

  let { tabs = [], value = $bindable(undefined), label = undefined, class: className = "", panel } = $props();

  const own = $props.id();
  const shown = $derived(tabs.filter((t) => !t.can || $canStore(t.can)));
  const current = $derived(shown.some((t) => t.id === value) ? value : shown.find((t) => !t.disabled)?.id);
  let list = $state();
  // The line under the shown tab: measured from it, so it can glide.
  let line = $state({ x: 0, width: 0 });
  let gliding = $state(false);

  $effect(() => {
    if (!list) return;
    void current;
    void shown.length;
    const measure = () => {
      const tab = list.querySelector('[role="tab"][aria-selected="true"]');
      line = tab ? { x: tab.offsetLeft + 8, width: Math.max(tab.offsetWidth - 16, 0) } : { x: 0, width: 0 };
    };
    measure();
    // Placed before it glides: the first one doesn't slide in from the left.
    const start = requestAnimationFrame(() => (gliding = true));
    const resized = typeof ResizeObserver === "function" ? new ResizeObserver(measure) : undefined;
    resized?.observe(list);
    return () => {
      cancelAnimationFrame(start);
      resized?.disconnect();
    };
  });

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
    <span
      class="line"
      class:gliding
      aria-hidden="true"
      style:width="{line.width}px"
      style:transform="translateX({line.x}px)"
    ></span>
  </div>
  <div id="{own}-panel" role="tabpanel" aria-labelledby="{own}-tab-{current}" tabindex="0" class="panel">
    {#if current !== undefined}{@render panel?.(current)}{/if}
  </div>
</div>

<style>
  .elyra-tabs { display: grid; gap: 14px; font: 13px var(--elyra-font); color: var(--elyra-text); min-width: 0; }
  [role="tablist"] {
    position: relative; display: flex; gap: 2px; border-bottom: 1px solid var(--elyra-border); outline: none; overflow-x: auto;
  }
  [role="tab"] {
    height: 34px; padding: 0 12px; border: 0; background: transparent;
    color: var(--elyra-muted); font: 500 13px var(--elyra-font); cursor: pointer; white-space: nowrap;
  }
  [role="tab"]:hover:not(:disabled) { color: var(--elyra-text); }
  [role="tab"] { transition: color var(--elyra-duration-fast) var(--elyra-ease); }
  [role="tab"][aria-selected="true"] { color: var(--elyra-text); }
  .line {
    position: absolute; left: 0; bottom: 0; height: 2px; border-radius: 2px; background: var(--elyra-accent);
    pointer-events: none;
  }
  .line.gliding {
    transition: transform var(--elyra-duration) var(--elyra-ease-spring), width var(--elyra-duration) var(--elyra-ease-spring);
  }
  /* Windows' high contrast: the line in the system's highlight colour. */
  @media (forced-colors: active) { .line { background: Highlight; forced-color-adjust: none; } }
  [role="tab"]:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: -2px; border-radius: 4px; }
  [role="tab"]:disabled { opacity: .45; cursor: default; }
  .panel { outline: none; }
  .panel:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; border-radius: 4px; }
</style>
