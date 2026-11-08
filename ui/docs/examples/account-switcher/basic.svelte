<script>
  import { alert } from "@elyra/runtime";
  import { AccountSwitcher, NavItem, Sidebar } from "@elyra/ui";
</script>

<!-- Ada works at Acme and for a customer, Kunde: one click moves between them. -->
<div class="window">
  <Sidebar>
    {#snippet header()}
      <!-- Signing in stays the app's: its own form opens from here. -->
      <AccountSwitcher onadd={() => alert("The app's sign-in form opens here.")} />
    {/snippet}
    <NavItem path="/customers">Customers</NavItem>
    <NavItem path="/invoices">Invoices</NavItem>
  </Sidebar>
</div>

<style>
  .window { display: grid; height: 380px; width: min(100%, 280px); border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); }
</style>
