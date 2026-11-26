<script>
  import { Button, EditingDrawer } from "@elyra/ui";

  let open = $state(false);
</script>

<Button onclick={() => (open = true)}>Show files</Button>
<EditingDrawer bind:open />
