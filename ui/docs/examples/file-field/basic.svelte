<script>
  import { Field, FileField } from "@elyra/ui";

  let receipt = $state(null);
</script>

<Field label="Receipt" description="A PDF or a photo of it.">
  <FileField
    bind:file={receipt}
    filters={[{ name: "Receipts", extensions: ["pdf", "jpg", "png"] }]}
    acceptsDrop
  />
</Field>
