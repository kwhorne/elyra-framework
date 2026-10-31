<script>
  import { Field, Input } from "@elyra/ui";

  let name = $state("");
</script>

<Field label="Name">
  <Input bind:value={name} placeholder="Ada Lovelace" autocomplete="name" />
</Field>
