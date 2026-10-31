<script>
  import { AppShell, Button, Empty } from "@elyra/ui";

  // In an app: `import { nav } from "./resources/index.js"`, which
  // make:resource keeps.
  const nav = [
    { label: "Overview", path: "/" },
    { label: "Customers", path: "/customers", count: 3 },
    { label: "Invoices", path: "/invoices", can: "invoices.view" },
    { label: "Settings", path: "/settings" },
  ];
</script>

<!-- The shell fills the window; here it's in a box. -->
<div class="window">
  <AppShell title="CRM" {nav}>
    {#snippet actions()}
      <Button size="sm" variant="primary" href="/customers/new">New customer</Button>
    {/snippet}
    <Empty title="Pick a customer" description="Or press ⌘K and type a name." />
  </AppShell>
</div>

<style>
  .window { width: 100%; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden; }
  .window :global(.elyra-app-shell) { height: 340px; }
</style>
