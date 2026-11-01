<script>
  import { DatePicker, Field } from "@elyra/ui";

  let from = $state("2026-11-02");
  let to = $state(null);
</script>

<!-- `min` and `max` bound each other: the end can't come before the start. -->
<div class="range">
  <Field label="From"><DatePicker bind:value={from} max={to ?? undefined} /></Field>
  <Field label="To"><DatePicker bind:value={to} min={from ?? undefined} /></Field>
</div>

<style>
  .range { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
</style>
