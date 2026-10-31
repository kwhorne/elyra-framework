<script>
  import { Combobox, Field } from "@elyra/ui";

  let city = $state("Bergen");
  const CITIES = ["Bergen", "Bodø", "Drammen", "Kristiansand", "Oslo", "Stavanger", "Tromsø", "Trondheim"];
</script>

<Field label="City">
  <Combobox bind:value={city} options={CITIES} placeholder="Type a city" />
</Field>
