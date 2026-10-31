<script>
  import Field from "../Field.svelte";
  import Select from "../Select.svelte";

  let { value = $bindable(undefined), ...props } = $props();
</script>

<Field label="Country"><Select bind:value {...props} /></Field>
<button type="button">Elsewhere</button>
