<script>
  import { Field, Textarea } from "@elyra/ui";

  let notes = $state("Prefers invoices by email.");
</script>

<Field label="Notes" description="Everyone who can see the customer sees these.">
  <Textarea bind:value={notes} rows={3} />
</Field>
