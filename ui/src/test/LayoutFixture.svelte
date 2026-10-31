<script>
  import AppShell from "../AppShell.svelte";
  import Tabs from "../Tabs.svelte";
  import Card from "../Card.svelte";
  import Callout from "../Callout.svelte";
  import Badge from "../Badge.svelte";
  import Button from "../Button.svelte";

  let { nav = [], layout = "sidebar", tab = $bindable("details"), ondismiss = undefined } = $props();
</script>

<AppShell title="CRM" {nav} {layout}>
  {#snippet actions()}<Button size="sm">New customer</Button>{/snippet}
  <Card title="Ada Lovelace" description="Customer since 2021">
    {#snippet actions()}<Badge variant="success">Active</Badge>{/snippet}
    <Tabs
      bind:value={tab}
      label="Customer"
      tabs={[
        { id: "details", label: "Details" },
        { id: "invoices", label: "Invoices" },
        { id: "notes", label: "Notes", disabled: true },
        { id: "audit", label: "Audit", can: "customers.audit" },
        { id: "files", label: "Files" },
      ]}
    >
      {#snippet panel(id)}<p>Panel: {id}</p>{/snippet}
    </Tabs>
  </Card>
  <Callout variant="danger" title="Overdue" dismissible {ondismiss}>Two invoices are overdue.</Callout>
  <Callout title="Tip">Press ⌘K to search.</Callout>
</AppShell>
