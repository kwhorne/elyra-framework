<script>
  import Form from "../Form.svelte";
  import Field from "../Field.svelte";
  import Input from "../Input.svelte";
  import Button from "../Button.svelte";

  let { onsubmit, name = $bindable(""), email = $bindable("") } = $props();
</script>

<Form {onsubmit}>
  <Field name="name" label="Name" description="As on the invoice." required>
    <Input bind:value={name} />
  </Field>
  <Field name="email" label="Email">
    <Input type="email" bind:value={email} />
  </Field>
  <Button type="submit" variant="primary">Save</Button>
</Form>
