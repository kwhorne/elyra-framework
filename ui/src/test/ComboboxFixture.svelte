<script>
  import Field from "../Field.svelte";
  import Combobox from "../Combobox.svelte";

  let { value = $bindable(null), label = $bindable(""), ...props } = $props();
</script>

<Field label="Customer"><Combobox bind:value bind:label {...props} /></Field>
<button type="button">Elsewhere</button>
<p data-testid="value">{JSON.stringify(value)}</p>
<p data-testid="label">{label}</p>
