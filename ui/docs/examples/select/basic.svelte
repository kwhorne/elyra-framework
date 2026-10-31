<script>
  import { Field, Select } from "@elyra/ui";

  let country = $state("Norway");
</script>

<Field label="Country">
  <Select bind:value={country} options={["Denmark", "Finland", "Iceland", "Norway", "Sweden"]} />
</Field>
