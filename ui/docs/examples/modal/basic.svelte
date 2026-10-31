<script>
  import { Button, Modal } from "@elyra/ui";

  let open = $state(false);
</script>

<Button variant="danger" onclick={() => (open = true)}>Delete customer</Button>

<Modal bind:open title="Delete Ada Lovelace?" description="Her 12 invoices stay in the accounts." size="sm">
  <p>This can't be undone.</p>
  {#snippet footer()}
    <Button onclick={() => (open = false)}>Cancel</Button>
    <Button variant="danger" onclick={() => (open = false)}>Delete</Button>
  {/snippet}
</Modal>
