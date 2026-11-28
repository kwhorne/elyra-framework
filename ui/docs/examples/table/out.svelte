<script>
  import { Button, Table } from "@elyra/ui";

  const INVOICES = [
    { id: 2041, number: "2041", customer: "Ada Lovelace", due: "2026-11-05", amount: 12400.5 },
    { id: 2042, number: "2042", customer: "Grace Hopper", due: "2026-11-07", amount: 860 },
    { id: 2043, number: "2043", customer: "Kristen Nygaard", due: "2026-11-12", amount: 3150 },
  ];
  const euros = (n) => new Intl.NumberFormat("en", { style: "currency", currency: "EUR" }).format(n);

  let table = $state();
  let selected = $state([]);
</script>

<!-- Right-click a row, or ⌘C on one: the amounts go as 12400.5, not €12,400.50. -->
<Table
  bind:this={table}
  rows={INVOICES}
  bind:selected
  select
  label="Invoices"
  columns={["number", "customer", "due", { key: "amount", align: "end", format: euros }]}
/>
<Button onclick={() => table.save()}>Save as CSV…</Button>
