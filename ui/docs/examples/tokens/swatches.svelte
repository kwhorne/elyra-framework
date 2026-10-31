<script>
  // The tokens' values, read from the page: switch the theme to see the other set.
  const NAMES = [
    "bg", "surface", "raised", "inset", "text", "muted", "border", "focus",
    "accent", "accent-soft", "on-accent", "danger", "success", "warning",
  ];
  let values = $state({});

  $effect(() => {
    const read = () => {
      const style = getComputedStyle(document.documentElement);
      values = Object.fromEntries(NAMES.map((n) => [n, style.getPropertyValue(`--elyra-${n}`).trim()]));
    };
    read();
    const watch = new MutationObserver(read);
    watch.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => watch.disconnect();
  });
</script>

<div class="swatches">
  {#each NAMES as name (name)}
    <div class="swatch">
      <span class="chip" style:background="var(--elyra-{name})"></span>
      <code>--elyra-{name}</code>
      <small>{values[name]}</small>
    </div>
  {/each}
</div>

<style>
  .swatches { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr)); gap: 10px; width: 100%; }
  .swatch { display: grid; grid-template-columns: auto 1fr; column-gap: 10px; align-items: center; }
  .chip {
    grid-row: span 2; width: 34px; height: 34px; border-radius: var(--elyra-radius);
    box-shadow: inset 0 0 0 1px var(--elyra-border);
  }
  code { font: 12px var(--elyra-font-mono); }
  small { font: 11px var(--elyra-font-mono); color: var(--elyra-muted); }
</style>
