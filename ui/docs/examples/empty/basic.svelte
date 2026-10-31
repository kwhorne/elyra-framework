<script>
  import { Button, Empty } from "@elyra/ui";
</script>

<Empty title="No notes yet" description="Notes are visible to everyone who can see this customer.">
  {#snippet action()}
    <Button size="sm" variant="primary">Add a note</Button>
  {/snippet}
</Empty>
