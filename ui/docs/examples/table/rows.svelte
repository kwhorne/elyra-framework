<script>
  import { Badge, Table } from "@elyra/ui";

  const CUSTOMERS = [
    { id: 1, name: "Ada Lovelace", email: "ada@analytical.co", credit: 12400, status: "Active" },
    { id: 2, name: "Grace Hopper", email: "grace@cobol.dev", credit: 860, status: "Overdue" },
    { id: 3, name: "Linus Torvalds", email: "linus@kernel.org", credit: 3150, status: "Active" },
    { id: 4, name: "Margaret Hamilton", email: "margaret@apollo.space", credit: 22000, status: "Paused" },
    { id: 5, name: "Edsger Dijkstra", email: "edsger@goto.nl", credit: 0, status: "Active" },
    { id: 6, name: "Barbara Liskov", email: "barbara@clu.mit.edu", credit: 4700, status: "Active" },
  ];
  const TONE = { Active: "success", Overdue: "danger", Paused: "warning" };
  const euros = (n) => new Intl.NumberFormat("en", { style: "currency", currency: "EUR", maximumFractionDigits: 0 }).format(n);

  let query = $state({});
  let opened = $state(null);
</script>

<Table
  rows={CUSTOMERS}
  bind:query
  perPage={4}
  search
  sortable={["name", "credit"]}
  label="Customers"
  columns={["name", "email", { key: "credit", align: "end", format: euros }, "status"]}
  onrowclick={(row) => (opened = row.name)}
>
  {#snippet cell(row, column)}
    {#if column.key === "status"}
      <Badge variant={TONE[row.status]}>{row.status}</Badge>
    {:else}
      {column.format ? column.format(row[column.key], row) : row[column.key]}
    {/if}
  {/snippet}
</Table>
<p>Opened: {opened ?? "none"} · query: <code>{JSON.stringify(query)}</code></p>
