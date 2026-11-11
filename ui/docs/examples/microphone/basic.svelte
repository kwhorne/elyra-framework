<script>
  import { Field, Microphone, Textarea } from "@elyra/ui";

  let notes = $state("");
  const add = (text) => (notes = notes ? `${notes} ${text}` : text);
</script>

<Field label="Notes" description="Type, or dictate.">
  <Textarea bind:value={notes} rows={3} />
</Field>
<Microphone language="en" ontext={add} />
