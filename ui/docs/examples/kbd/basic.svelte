<script>
  import { Kbd } from "@elyra/ui";
</script>

<!-- As this window's platform writes them: ⌘ on a Mac, Ctrl elsewhere. -->
<ul class="keys">
  <li><Kbd keys="CmdOrCtrl+K" /> Search everything</li>
  <li><Kbd keys="CmdOrCtrl+Shift+P" /> Run a command</li>
  <li><Kbd keys="Alt+Up" /> Move the line up</li>
  <li><Kbd keys={["Shift", "Enter"]} /> A new line in the message</li>
  <li><Kbd keys="Esc" /> Close</li>
</ul>

<style>
  .keys { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; color: var(--elyra-text); }
  .keys li { display: flex; align-items: center; gap: 12px; }
  .keys :global(.elyra-kbd) { min-width: 96px; }
</style>
