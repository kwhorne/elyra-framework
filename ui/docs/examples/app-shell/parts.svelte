<script>
  import { writable } from "svelte/store";
  import { Badge, Button, NavItem, Separator, Sidebar, Toolbar } from "@elyra/ui";

  // A count can be a store: the unread notifications, say.
  const unread = writable(4);
</script>

<div class="window">
  <Toolbar title="CRM">
    {#snippet actions()}
      <Button size="sm" onclick={() => unread.update((n) => n + 1)}>Notify</Button>
    {/snippet}
  </Toolbar>
  <Sidebar>
    {#snippet header()}<Badge variant="accent">Analytical Engines</Badge>{/snippet}
    <NavItem path="/inbox" count={unread}>Inbox</NavItem>
    <NavItem path="/customers">
      {#snippet icon()}●{/snippet}
      Customers
    </NavItem>
    <NavItem path="/invoices" can="customers.delete">Hidden: no ability</NavItem>
    <Separator />
    <NavItem path="/settings">Settings</NavItem>
    {#snippet footer()}<small>Signed in as Ada</small>{/snippet}
  </Sidebar>
</div>

<style>
  .window {
    display: grid; grid-template-rows: auto 260px; width: 100%;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden;
  }
</style>
