<script>
  import { Button } from "@elyra/ui";
</script>

<!-- The user may export customers, but not delete them. -->
<Button can="customers.export">Export</Button>
<Button variant="danger" can="customers.delete">Delete (hidden)</Button>
<Button variant="danger" can="customers.delete" canMode="disable">Delete</Button>
