<script>
  import { Button, Table } from "@elyra/ui";

  let customers = $state([
    { id: 1, name: "Ada Lovelace", email: "ada@analytical.co" },
    { id: 2, name: "Grace Hopper", email: "grace@cobol.dev" },
  ]);
</script>

<Table rows={customers} columns={["name", "email"]} label="Customers">
  {#snippet actions(row)}
    <Button size="sm" href="/customers/{row.id}">Open</Button>
    <Button
      size="sm"
      variant="danger"
      confirm="Delete {row.name}?"
      onclick={() => (customers = customers.filter((c) => c.id !== row.id))}
    >
      Delete
    </Button>
  {/snippet}
  {#snippet empty()}
    <p>Everyone is gone. Reload the page to bring them back.</p>
  {/snippet}
</Table>
