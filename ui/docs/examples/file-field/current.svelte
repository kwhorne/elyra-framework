<script>
  import { Field, FileField } from "@elyra/ui";

  // Editing: the record has a contract already. A new one replaces it on save.
  const invoice = { id: 2041, contract: "contract-2026.pdf" };
  let contract = $state(null);
</script>

<Field label="Contract" description={contract ? "Replaces the current one on save." : undefined}>
  <FileField bind:file={contract} current={invoice.contract} />
</Field>
