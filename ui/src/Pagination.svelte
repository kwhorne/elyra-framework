<!--
  Pages of a list: Previous, Next, and the numbers around the current one.
  `bind:page` (1-based) and `lastPage`; `onchange` is told the new page.
  `previousLabel`, `nextLabel` and `pageLabel(n)` translate it.
-->
<script>
  let {
    page = $bindable(1),
    lastPage = 1,
    label = "Pagination",
    previousLabel = "Previous",
    nextLabel = "Next",
    pageLabel = (n) => `Page ${n}`,
    onchange = undefined,
    class: className = "",
  } = $props();

  /** The first, the last, and the current one with its neighbours; `null` is a gap. */
  const pages = $derived.by(() => {
    const keep = new Set([1, lastPage, page - 1, page, page + 1].filter((p) => p >= 1 && p <= lastPage));
    const sorted = [...keep].sort((a, b) => a - b);
    const out = [];
    for (const p of sorted) {
      if (out.length && p - out[out.length - 1] > 1) out.push(null);
      out.push(p);
    }
    return out;
  });

  function go(to) {
    if (to < 1 || to > lastPage || to === page) return;
    page = to;
    onchange?.(to);
  }
</script>

{#if lastPage > 1}
  <nav aria-label={label} class="elyra-pagination {className}">
    <button type="button" class="step" disabled={page <= 1} onclick={() => go(page - 1)}>{previousLabel}</button>
    {#each pages as p, i (p ?? `gap-${i}`)}
      {#if p === null}
        <span class="gap" aria-hidden="true">…</span>
      {:else}
        <button
          type="button"
          class="number"
          aria-label={pageLabel(p)}
          aria-current={p === page ? "page" : undefined}
          onclick={() => go(p)}>{p}</button
        >
      {/if}
    {/each}
    <button type="button" class="step" disabled={page >= lastPage} onclick={() => go(page + 1)}>{nextLabel}</button>
  </nav>
{/if}

<style>
  .elyra-pagination { display: flex; align-items: center; gap: 4px; font: 13px var(--elyra-font); font-variant-numeric: tabular-nums; }
  button {
    height: 28px; min-width: 28px; padding: 0 8px; box-sizing: border-box;
    border: 1px solid transparent; border-radius: var(--elyra-radius);
    background: transparent; color: var(--elyra-text); font: inherit; cursor: pointer;
  }
  button:hover:not(:disabled) { background: var(--elyra-inset); }
  button:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  button:disabled { color: var(--elyra-muted); opacity: .6; cursor: default; }
  .step { border-color: var(--elyra-border); }
  [aria-current="page"] { background: var(--elyra-accent-soft); color: var(--elyra-accent); font-weight: 600; }
  .gap { color: var(--elyra-muted); padding: 0 2px; }
</style>
