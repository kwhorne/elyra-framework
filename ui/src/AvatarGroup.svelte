<!--
  A few people at once, overlapping: the first `max`, and a count of the
  rest, whose names are its label and tooltip. `people` is a list of
  `{ name, src }`.
-->
<script>
  import Avatar from "./Avatar.svelte";
  import { avatarSize } from "./internal/people.js";

  let { people = [], max = 4, size = "md", label = "People", class: className = "", ...rest } = $props();

  // Past `max`, the last place is the count: it never shows max + 1 things.
  const cut = $derived(people.length > max ? Math.max(max - 1, 0) : people.length);
  const shown = $derived(people.slice(0, cut));
  const others = $derived(people.slice(cut));
  const othersNames = $derived(others.map((p) => p.name).join(", "));
</script>

<div class="elyra-avatar-group {className}" role="group" aria-label={label} {...rest}>
  {#each shown as person, i (person.id ?? `${person.name}-${i}`)}
    <span class="ring" title={person.name}><Avatar name={person.name} src={person.src} {size} /></span>
  {/each}
  {#if others.length}
    <span class="ring" title={othersNames}>
      <span class="more" style:--size="{avatarSize(size)}px" role="img" aria-label="{others.length} more: {othersNames}">
        <span aria-hidden="true">+{others.length}</span>
      </span>
    </span>
  {/if}
</div>

<style>
  .elyra-avatar-group { display: inline-flex; align-items: center; }
  .ring {
    display: inline-flex; border-radius: 50%;
    box-shadow: 0 0 0 2px var(--elyra-surface);
  }
  .ring + .ring { margin-left: -6px; }
  .more {
    display: inline-grid; place-items: center; width: var(--size); height: var(--size); border-radius: 50%;
    background: var(--elyra-inset); color: var(--elyra-muted);
    font: 600 calc(var(--size) * 0.36) / 1 var(--elyra-font); font-variant-numeric: tabular-nums;
  }
</style>
