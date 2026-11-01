<script>
  import { DatePicker, Field } from "@elyra/ui";

  let due = $state("2026-11-30");
</script>

<!-- The value is ISO whatever the locale; the field writes it the user's way. -->
<Field label="Due" description="Type it, or pick it in the calendar.">
  <DatePicker bind:value={due} />
</Field>
<p class="value">value: {JSON.stringify(due)}</p>

<style>
  .value { margin: 8px 0 0; color: var(--elyra-muted); font: 12px var(--elyra-font-mono); }
</style>
