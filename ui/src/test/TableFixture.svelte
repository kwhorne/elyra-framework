<script>
  import Table from "../Table.svelte";
  import Button from "../Button.svelte";

  let { query = $bindable({}), withActions = false, onopen = undefined, ...props } = $props();
</script>

<Table bind:query {...props} onrowclick={onopen}>
  {#snippet actions(row)}
    {#if withActions}<Button size="sm" onclick={() => onopen?.(row)}>Open {row.name}</Button>{/if}
  {/snippet}
</Table>
<output data-testid="query">{JSON.stringify(query)}</output>
