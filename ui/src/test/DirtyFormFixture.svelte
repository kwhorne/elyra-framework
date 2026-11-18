<script>
  import Form from "../Form.svelte";
  import Field from "../Field.svelte";
  import Input from "../Input.svelte";
  import Select from "../Select.svelte";
  import Switch from "../Switch.svelte";
  import Button from "../Button.svelte";

  let { onsubmit, oninput = undefined, dirty = $bindable(false) } = $props();
  let name = $state("");
  let country = $state(undefined);
  let active = $state(false);
  const countries = [
    { value: "no", label: "Norway" },
    { value: "se", label: "Sweden" },
  ];
</script>

<Form {onsubmit} {oninput} bind:dirty>
  <Field name="name" label="Name"><Input bind:value={name} /></Field>
  <Field name="country" label="Country"><Select bind:value={country} options={countries} searchable /></Field>
  <Field name="active" label="Active"><Switch bind:checked={active} /></Field>
  <Button type="submit" variant="primary">Save</Button>
</Form>
<output data-testid="dirty">{dirty}</output>
