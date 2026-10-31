<script>
  import Dropdown from "../Dropdown.svelte";
  import MenuItem from "../MenuItem.svelte";

  let { onedit, ondelete } = $props();
</script>

<Dropdown label="Actions">
  <MenuItem onclick={onedit}>Edit</MenuItem>
  <MenuItem disabled>Archive</MenuItem>
  <MenuItem can="customers.export">Export</MenuItem>
  <MenuItem can="customers.delete" canMode="disable" danger onclick={ondelete}>Delete</MenuItem>
</Dropdown>
<button type="button">Elsewhere</button>
