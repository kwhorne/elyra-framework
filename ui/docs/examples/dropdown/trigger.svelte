<script>
  import { Badge, Dropdown, MenuItem } from "@elyra/ui";
</script>

<Dropdown variant="ghost" align="end">
  {#snippet trigger()}
    <Badge variant="accent">AL</Badge> Ada Lovelace
  {/snippet}
  <MenuItem>Profile</MenuItem>
  <MenuItem>Keyboard shortcuts</MenuItem>
  <MenuItem danger>Sign out</MenuItem>
</Dropdown>
