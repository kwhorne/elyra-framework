<script>
  import { Microphone } from "@elyra/ui";

  let said = $state("");
</script>

<Microphone
  language="no"
  labels={{
    button: "Dikter",
    recording: "Tar opp",
    transcribing: "Skriver ut…",
    hint: "Klikk, eller hold mellomrom, for å snakke",
  }}
  ontext={(text) => (said = text)}
/>
<p>{said}</p>
