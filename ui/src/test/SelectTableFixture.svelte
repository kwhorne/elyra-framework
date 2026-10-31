<script>
  import Table from "../Table.svelte";
  import Button from "../Button.svelte";

  let {
    selected = $bindable([]),
    query = $bindable({}),
    shown = true,
    ondelete = undefined,
    editable = false,
    ...props
  } = $props();
</script>

{#if shown}
  <Table bind:selected bind:query columns={["name", "city"]} label="Customers" {...props}>
    {#snippet cell(row, column)}
      {#if editable && column.key === "city"}<input aria-label="City of {row.name}" value={row.city} />{:else}{row[column.key]}{/if}
    {/snippet}
    {#snippet bulk(ids, clear)}
      <Button size="sm" onclick={() => (ondelete?.(ids), clear())}>Delete {ids.length}</Button>
    {/snippet}
  </Table>
{/if}
<output data-testid="selected">{JSON.stringify(selected)}</output>
