<script>
  import { Table } from "@elyra/ui";

  const INVOICES = [
    { id: 2041, customer: "Ada Lovelace", status: "open", amount: 12400 },
    { id: 2042, customer: "Grace Hopper", status: "paid", amount: 860 },
    { id: 2043, customer: "Kristen Nygaard", status: "overdue", amount: 3150 },
    { id: 2044, customer: "Ole-Johan Dahl", status: "open", amount: 22000 },
  ];
  const STATUS = [["open", "Open"], ["paid", "Paid"], ["overdue", "Overdue"]];
  const VIEWS = [
    { name: "Overdue", filters: { status: ["overdue"] } },
    { name: "Large", filters: { amount: { min: 10000 } }, sort: "amount", direction: "desc" },
  ];

  let view = $state(null);
</script>

<!-- Views: pick one, change a filter ("Large (changed)"), save your own. -->
<Table
  rows={INVOICES}
  label="Invoices"
  views={VIEWS}
  bind:view
  persist="docs-invoices-views"
  columns={[
    { key: "customer", filter: "text", sortable: true },
    { key: "status", filter: { options: STATUS } },
    { key: "amount", align: "end", filter: "range", sortable: true },
  ]}
/>
<p>View: {view ?? "All"}</p>
