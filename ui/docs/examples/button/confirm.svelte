<script>
  import { Button } from "@elyra/ui";

  let deleted = $state(false);
</script>

<Button
  variant="danger"
  confirm="Delete Ada Lovelace? Her 12 invoices stay in the accounts."
  confirmLabel="Delete customer"
  onclick={() => (deleted = true)}
>
  Delete customer
</Button>
{#if deleted}<span>Deleted.</span>{/if}
