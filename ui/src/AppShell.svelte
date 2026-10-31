<!--
  The window: a toolbar with the app's name and `actions`, its navigation
  (down the side, or `layout="topbar"` across the top), the page, and the
  status bar. `nav` is the resources' registry (`[{ label, path, count }]`),
  or put `<NavItem>`s in a `navigation` snippet.
-->
<script>
  import NavItem from "./NavItem.svelte";
  import Sidebar from "./Sidebar.svelte";
  import StatusBar from "./StatusBar.svelte";
  import Toolbar from "./Toolbar.svelte";

  let {
    title = undefined,
    nav = [],
    layout = "sidebar",
    statusBar = true,
    class: className = "",
    actions = undefined,
    navigation = undefined,
    children,
  } = $props();
</script>

{#snippet links()}
  {#if navigation}
    {@render navigation()}
  {:else}
    {#each nav as item (item.path)}
      <NavItem path={item.path} count={item.count} can={item.can}>{item.label}</NavItem>
    {/each}
  {/if}
{/snippet}

<div class="elyra-app-shell {layout} {className}">
  <Toolbar {title} {actions}>
    {#if layout === "topbar"}<nav class="topnav" aria-label="Main">{@render links()}</nav>{/if}
  </Toolbar>
  <div class="middle">
    {#if layout === "sidebar"}<Sidebar>{@render links()}</Sidebar>{/if}
    <main class="content">{@render children?.()}</main>
  </div>
  {#if statusBar}<StatusBar />{/if}
</div>

<style>
  .elyra-app-shell {
    display: flex; flex-direction: column; height: 100vh; overflow: hidden;
    background: var(--elyra-bg); color: var(--elyra-text); font: var(--elyra-font-size)/1.5 var(--elyra-font);
  }
  .middle { display: flex; flex: 1; min-height: 0; }
  .content { flex: 1; min-width: 0; overflow: auto; padding: 24px; }
  .topnav { display: flex; gap: 4px; margin-left: 12px; }
</style>
