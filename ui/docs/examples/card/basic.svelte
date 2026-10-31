<script>
  import { Badge, Button, Card } from "@elyra/ui";
</script>

<Card title="Ada Lovelace" description="Customer since March 2021 · Analytical Engines Ltd">
  {#snippet actions()}
    <Badge variant="success">Active</Badge>
    <Button size="sm">Edit</Button>
  {/snippet}
  <p>12 invoices, 1 860 € overdue. Prefers invoices by email.</p>
  {#snippet footer()}
    <Button size="sm" variant="ghost" href="/customers/1/invoices">All invoices</Button>
  {/snippet}
</Card>
