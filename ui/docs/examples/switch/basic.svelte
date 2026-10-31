<script>
  import { Switch } from "@elyra/ui";

  let autostart = $state(true);
  let saved = $state("");
</script>

<Switch bind:checked={autostart} onchange={(on) => (saved = on ? "Starts with the computer." : "Won't start on its own.")}>
  Open at login
</Switch>
<Switch disabled>Beta updates</Switch>
<span>{saved}</span>
