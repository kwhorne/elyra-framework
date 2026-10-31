<script>
  import { Button, Table } from "@elyra/ui";

  const INVOICES = [
    { id: 2041, number: "2041", customer: "Ada Lovelace", due: "2026-11-05", amount: 12400 },
    { id: 2042, number: "2042", customer: "Grace Hopper", due: "2026-11-07", amount: 860 },
    { id: 2043, number: "2043", customer: "Kristen Nygaard", due: "2026-11-12", amount: 3150 },
    { id: 2044, number: "2044", customer: "Ole-Johan Dahl", due: "2026-11-14", amount: 22000 },
    { id: 2045, number: "2045", customer: "Margaret Hamilton", due: "2026-11-20", amount: 4700 },
  ];
  const euros = (n) => new Intl.NumberFormat("en", { style: "currency", currency: "EUR", maximumFractionDigits: 0 }).format(n);

  let selected = $state([]);
  let sent = $state([]);
</script>

<!-- `select="invoices"`: the assistant's note says which are selected too. -->
<Table
  rows={INVOICES}
  bind:selected
  select="invoices"
  perPage={4}
  label="Invoices"
  columns={["number", "customer", "due", { key: "amount", align: "end", format: euros }]}
>
  {#snippet bulk(ids, clear)}
    <Button size="sm" variant="primary" onclick={() => ((sent = ids), clear())}>Send reminders</Button>
  {/snippet}
</Table>
<p>Selected: <code>{JSON.stringify(selected)}</code> · reminded: <code>{JSON.stringify(sent)}</code></p>
