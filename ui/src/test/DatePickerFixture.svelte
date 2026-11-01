<script>
  import Field from "../Field.svelte";
  import DatePicker from "../DatePicker.svelte";

  let { value = $bindable(null), ...props } = $props();
</script>

<Field label="Due"><DatePicker bind:value {...props} /></Field>
<button type="button">Elsewhere</button>
<p data-testid="value">{JSON.stringify(value)}</p>
