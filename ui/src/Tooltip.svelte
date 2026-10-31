<!--
  A short hint for what's inside, on hover or keyboard focus: `role="tooltip"`,
  and the element inside is described by it. Esc hides it.
-->
<script>
  let { text, placement = "top", delay = 400, class: className = "", children } = $props();

  const own = $props.id();
  let anchor = $state();
  let shown = $state(false);
  let timer;

  // The element inside is what a screen reader focuses: it carries the hint.
  $effect(() => {
    const target = anchor?.firstElementChild;
    if (!target) return;
    target.setAttribute("aria-describedby", `${own}-tip`);
    return () => target.removeAttribute("aria-describedby");
  });

  function show(now = false) {
    clearTimeout(timer);
    if (now) shown = true;
    else timer = setTimeout(() => (shown = true), delay);
  }

  function hide() {
    clearTimeout(timer);
    shown = false;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<span
  bind:this={anchor}
  class="elyra-tooltip {className}"
  onmouseenter={() => show()}
  onmouseleave={hide}
  onfocusin={() => show(true)}
  onfocusout={hide}
  onkeydown={(event) => event.key === "Escape" && hide()}
>
  {@render children?.()}
  <span id="{own}-tip" role="tooltip" class="tip {placement}" hidden={!shown}>{text}</span>
</span>

<style>
  .elyra-tooltip { position: relative; display: inline-flex; }
  .tip {
    position: absolute; z-index: 70; left: 50%; transform: translateX(-50%); bottom: calc(100% + 6px);
    padding: 4px 8px; border-radius: 6px; white-space: nowrap; pointer-events: none;
    background: var(--elyra-text); color: var(--elyra-bg); font: 12px/1.4 var(--elyra-font);
  }
  .tip.bottom { bottom: auto; top: calc(100% + 6px); }
  .tip[hidden] { display: none; }
</style>
