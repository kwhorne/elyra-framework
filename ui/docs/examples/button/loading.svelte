<script>
  import { Button } from "@elyra/ui";

  let syncing = $state(false);

  async function sync() {
    syncing = true;
    await new Promise((resolve) => setTimeout(resolve, 1500));
    syncing = false;
  }
</script>

<Button variant="primary" loading={syncing} onclick={sync}>Sync now</Button>
<Button disabled>Archived</Button>
