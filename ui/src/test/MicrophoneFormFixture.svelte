<script>
  import Form from "../Form.svelte";
  import Field from "../Field.svelte";
  import Textarea from "../Textarea.svelte";
  import Microphone from "../Microphone.svelte";
  import Button from "../Button.svelte";

  let { onsubmit = async () => {}, error = undefined, dirty = $bindable(false) } = $props();
  let notes = $state("");
  let memo = $state(null);
</script>

<Form {onsubmit} bind:dirty>
  <Field name="notes" label="Notes">
    <Textarea bind:value={notes} />
    <Microphone ontext={(text) => (notes = text)} labels={{ button: "Dictate notes" }} />
  </Field>
  <Field name="memo" label="Voice memo" description="Up to a minute." {error}>
    <Microphone transcribe={false} onrecorded={(file) => (memo = file)} />
  </Field>
  <Button type="submit">Save</Button>
</Form>
<output data-testid="dirty">{dirty}</output>
<output data-testid="memo">{memo?.name ?? "none"}</output>
