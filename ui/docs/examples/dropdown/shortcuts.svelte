<script>
  import { Dropdown, MenuItem } from "@elyra/ui";
</script>

<Dropdown label="File">
  <MenuItem shortcut="CmdOrCtrl+N">New customer</MenuItem>
  <MenuItem shortcut="CmdOrCtrl+Shift+E">Export…</MenuItem>
  <MenuItem shortcut="CmdOrCtrl+Backspace" danger>Delete</MenuItem>
</Dropdown>
