<script>
  import Form from "../Form.svelte";
  import Field from "../Field.svelte";
  import FileField from "../FileField.svelte";
  import Button from "../Button.svelte";

  let { file = $bindable(null), onsubmit = async () => {}, error = undefined, shown = true, ...props } = $props();
</script>

<Form {onsubmit}>
  {#if shown}
    <Field name="receipt" label="Receipt" description="A PDF or a photo." {error}>
      <FileField bind:file {...props} />
    </Field>
  {/if}
  <Button type="submit">Save</Button>
</Form>
<p data-testid="file">{file?.name ?? "none"}</p>
