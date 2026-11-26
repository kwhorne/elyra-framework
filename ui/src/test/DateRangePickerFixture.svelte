<script>
  import Field from "../Field.svelte";
  import DateRangePicker from "../DateRangePicker.svelte";

  let { start = $bindable(null), end = $bindable(null), ...props } = $props();
</script>

<Field label="Period"><DateRangePicker bind:start bind:end {...props} /></Field>
<button type="button">Elsewhere</button>
<p data-testid="value">{JSON.stringify({ start, end })}</p>
