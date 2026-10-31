<script>
  import SplitPane from "../SplitPane.svelte";

  let { size = $bindable(280), ...props } = $props();
</script>

<SplitPane bind:size {...props}>
  {#snippet first()}<p>Customers</p>{/snippet}
  {#snippet second()}<p>Ada Lovelace</p>{/snippet}
</SplitPane>
<p data-testid="size">{size}</p>
