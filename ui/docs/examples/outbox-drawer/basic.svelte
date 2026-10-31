<script>
  import { Button, OutboxDrawer } from "@elyra/ui";

  let open = $state(false);
</script>

<Button onclick={() => (open = true)}>Show changes</Button>
<OutboxDrawer bind:open />
