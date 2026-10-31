<script>
  import { Dropdown, MenuItem } from "@elyra/ui";

  let last = $state("nothing yet");
</script>

<Dropdown label="Actions">
  <MenuItem onclick={() => (last = "Edit")}>Edit</MenuItem>
  <MenuItem onclick={() => (last = "Duplicate")}>Duplicate</MenuItem>
  <MenuItem disabled>Archive</MenuItem>
  <MenuItem danger can="customers.delete" canMode="disable">Delete</MenuItem>
</Dropdown>
<span>Picked: {last}</span>
