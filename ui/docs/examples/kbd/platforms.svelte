<script>
  import { Kbd } from "@elyra/ui";
</script>

<!-- `platform` sets it, for a screenshot or a docs page about the other one. -->
<dl class="rows">
  <dt>macOS</dt>
  <dd><Kbd keys="CmdOrCtrl+Shift+E" platform="mac" /></dd>
  <dt>Windows and Linux</dt>
  <dd><Kbd keys="CmdOrCtrl+Shift+E" platform="other" /></dd>
</dl>

<style>
  .rows { display: grid; grid-template-columns: max-content auto; align-items: center; gap: 8px 16px; margin: 0; color: var(--elyra-text); }
  dd { margin: 0; }
</style>
