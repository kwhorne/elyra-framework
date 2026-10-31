<script>
  import { Callout, Empty, Tabs } from "@elyra/ui";

  let tab = $state("invoices");
  const TABS = [
    { id: "details", label: "Details" },
    { id: "invoices", label: "Invoices" },
    { id: "notes", label: "Notes" },
    { id: "audit", label: "Audit", can: "customers.delete" },
    { id: "files", label: "Files", disabled: true },
  ];
</script>

<Tabs bind:value={tab} tabs={TABS} label="Customer">
  {#snippet panel(id)}
    {#if id === "invoices"}
      <Callout variant="danger" title="2 invoices overdue">INV-2041 and INV-2047, 1 860 € in all.</Callout>
    {:else if id === "notes"}
      <Empty title="No notes yet" />
    {:else}
      <p>Ada Lovelace, Analytical Engines Ltd, London.</p>
    {/if}
  {/snippet}
</Tabs>
