<script>
  import { Table } from "@elyra/ui";

  const INVOICES = [
    { id: 2041, customer: "Ada Lovelace", status: "open", due: "2026-11-05", amount: 12400 },
    { id: 2042, customer: "Grace Hopper", status: "paid", due: "2026-11-07", amount: 860 },
    { id: 2043, customer: "Kristen Nygaard", status: "overdue", due: "2026-10-12", amount: 3150 },
    { id: 2044, customer: "Ole-Johan Dahl", status: "open", due: "2026-11-14", amount: 22000 },
    { id: 2045, customer: "Margaret Hamilton", status: "paid", due: "2026-11-20", amount: 4700 },
  ];
  const STATUS = [["open", "Open"], ["paid", "Paid"], ["overdue", "Overdue"]];
  const named = (s) => STATUS.find(([v]) => v === s)?.[1] ?? s;

  let query = $state({});
</script>

<!-- The funnel on a header filters by it; a chip says what's on, × takes it off. -->
<Table
  rows={INVOICES}
  bind:query
  label="Invoices"
  columns={[
    { key: "customer", filter: "text" },
    { key: "status", format: named, filter: { options: STATUS } },
    { key: "due", filter: "dates" },
    { key: "amount", align: "end", filter: "range", total: "sum" },
  ]}
/>
<pre>{JSON.stringify(query.filters ?? null)}</pre>
