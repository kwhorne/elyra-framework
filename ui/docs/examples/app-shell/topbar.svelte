<script>
  import { AppShell, NavItem, Skeleton } from "@elyra/ui";
</script>

<div class="window">
  <AppShell title="Notes" layout="topbar" statusBar={false}>
    {#snippet navigation()}
      <NavItem path="/notes">All notes</NavItem>
      <NavItem path="/customers">Customers</NavItem>
      <NavItem path="/archive">Archive</NavItem>
    {/snippet}
    <Skeleton lines={4} />
  </AppShell>
</div>

<style>
  .window { width: 100%; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden; }
  .window :global(.elyra-app-shell) { height: 220px; }
</style>
