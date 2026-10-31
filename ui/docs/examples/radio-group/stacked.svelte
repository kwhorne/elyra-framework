<script>
  import { RadioGroup } from "@elyra/ui";

  let delivery = $state("Email");
</script>

<RadioGroup label="Send invoices by" bind:value={delivery} options={["Email", "EHF", "Post"]} />
