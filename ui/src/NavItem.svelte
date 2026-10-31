<!--
  A link in the app's navigation. `path` is an app path (`/customers`): it
  is current (`aria-current="page"`) on that page and the pages under it.
  `count` is a number or a store of one (unread notifications); `can`
  hides it from a user without the ability.
-->
<script>
  import { can as canStore, href, route } from "@elyra/runtime";

  let { path, count = undefined, can = undefined, class: className = "", icon = undefined, children } = $props();

  const allowed = $derived(can ? $canStore(can) : true);
  const here = $derived($route.path === path || (path !== "/" && $route.path.startsWith(`${path}/`)));

  let counted = $state(0);
  $effect(() => {
    if (count?.subscribe) return count.subscribe((n) => (counted = n ?? 0));
    counted = count ?? 0;
  });
</script>

{#if allowed}
  <a href={href(path)} class="elyra-nav-item {className}" aria-current={here ? "page" : undefined}>
    {#if icon}<span class="icon">{@render icon()}</span>{/if}
    <span class="label">{@render children?.()}</span>
    {#if counted > 0}<span class="count">{counted > 99 ? "99+" : counted}</span>{/if}
  </a>
{/if}

<style>
  .elyra-nav-item {
    display: flex; align-items: center; gap: 8px; padding: 6px 10px; border-radius: var(--elyra-radius);
    color: var(--elyra-muted); text-decoration: none; font: 500 13px var(--elyra-font);
  }
  .elyra-nav-item:hover { color: var(--elyra-text); background: var(--elyra-inset); }
  .elyra-nav-item:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  .elyra-nav-item[aria-current="page"] { color: var(--elyra-text); background: var(--elyra-accent-soft); }
  .icon { display: inline-flex; }
  .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .count {
    min-width: 18px; padding: 0 6px; box-sizing: border-box; border-radius: 999px; text-align: center;
    background: var(--elyra-accent); color: var(--elyra-on-accent); font-size: 11px; font-weight: 600; line-height: 18px;
  }
</style>
