<script>
  import { DateRangePicker, Field } from "@elyra/ui";

  let start = $state("2026-11-02");
  let end = $state("2026-11-15");
</script>

<div class="range">
  <Field label="Period"><DateRangePicker bind:start bind:end /></Field>
  <p>{start ?? "…"} → {end ?? "…"}</p>
</div>

<style>
  .range { display: grid; gap: 8px; max-width: 320px; }
  p { margin: 0; color: var(--elyra-muted); font: 12px var(--elyra-font-mono); }
</style>
