<script>
  import { Report, Table } from "@elyra/ui";

  const lines = Array.from({ length: 60 }, (_, i) => ({ item: `Part ${i + 1}`, qty: (i % 4) + 1, price: 40 + ((i * 37) % 300) }));
</script>

<Report paper="a4" margins={{ top: 14, bottom: 12 }}>
  {#snippet header({ page })}
    {#if page === 1}<h1>Invoice 1042</h1><p>Acme AS · Storgata 1 · 0155 Oslo</p>{:else}<p>Invoice 1042</p>{/if}
  {/snippet}
  {#snippet footer({ page, pages })}
    <p class="foot">Acme AS · Org. 912 345 678 <span>Page {page} of {pages}</span></p>
  {/snippet}

  <p>Ada Lovelace · Analytical Engines Ltd</p>
  <Table rows={lines} columns={["item", "qty", "price"]} label="Lines" perPage={100} />
  <p><strong>Total: {lines.reduce((sum, l) => sum + l.qty * l.price, 0)} kr</strong></p>
</Report>

<style>
  .foot { display: flex; justify-content: space-between; color: var(--elyra-muted); font-size: 12px; }
</style>
