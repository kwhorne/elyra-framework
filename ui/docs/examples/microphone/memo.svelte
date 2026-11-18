<script>
  import { Field, Microphone } from "@elyra/ui";

  // Sent with the record on save, like a file the user chose.
  let memo = $state(null);
</script>

<Field label="Voice memo" description={memo ? memo.name : "Up to a minute."}>
  <Microphone transcribe={false} maxSeconds={60} can="memos.create" onrecorded={(file) => (memo = file)} />
</Field>
