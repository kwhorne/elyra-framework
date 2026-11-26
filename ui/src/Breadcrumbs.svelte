<!--
  Where this page is, from the top: `items`, each `{ label, path }` (an app
  path, through the router) or `{ label, href }` (anywhere else), the last
  being this page. A trail longer than `max` keeps the first and the last
  `max - 1` and folds the middle into "…", whose title says what's folded.
-->
<script>
  import { href as appHref } from "@elyra/runtime";
  import { useWords } from "./internal/words.js";

  let { items = [], max = 4, label = undefined, class: className = "" } = $props();
  const words = useWords("breadcrumbs");

  const shown = $derived.by(() => {
    if (items.length <= max || max < 2) return items.map((item) => ({ item }));
    const head = items.slice(0, 1);
    const tail = items.slice(items.length - (max - 1));
    const folded = items.slice(1, items.length - (max - 1));
    return [...head.map((item) => ({ item })), { folded }, ...tail.map((item) => ({ item }))];
  });
  const link = (item) => (item.path ? appHref(item.path) : item.href);
</script>

<nav class="elyra-breadcrumbs {className}" aria-label={label ?? words.label}>
  <ol>
    {#each shown as entry, i (i)}
      <li>
        {#if entry.folded}
          <span class="folded" title={entry.folded.map((f) => f.label).join(" › ")}>…</span>
        {:else if i === shown.length - 1}
          <span aria-current="page">{entry.item.label}</span>
        {:else if link(entry.item)}
          <a href={link(entry.item)}>{entry.item.label}</a>
        {:else}
          <span>{entry.item.label}</span>
        {/if}
      </li>
    {/each}
  </ol>
</nav>

<style>
  .elyra-breadcrumbs ol {
    display: flex; flex-wrap: wrap; align-items: center; gap: 4px; margin: 0; padding: 0; list-style: none;
    font: 13px var(--elyra-font); color: var(--elyra-muted);
  }
  li { display: inline-flex; align-items: center; gap: 4px; min-width: 0; }
  li + li::before { content: "›"; color: var(--elyra-muted); opacity: 0.7; }
  a { color: inherit; text-decoration: none; border-radius: 2px; }
  a:hover { color: var(--elyra-text); text-decoration: underline; text-underline-offset: 2px; }
  a:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  [aria-current="page"] { color: var(--elyra-text); font-weight: 600; }
  .folded { cursor: default; }
</style>
