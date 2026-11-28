<script>
  import { Report, Table } from "@elyra/ui";

  const lines = Array.from({ length: 60 }, (_, i) => {
    const qty = (i % 4) + 1;
    const price = 40 + ((i * 37) % 300);
    return { item: `Part ${i + 1}`, qty, price, amount: qty * price };
  });
  const kr = (n) => `${n.toLocaleString("nb")} kr`;
</script>

<Report paper="a4" margins={{ top: 14, bottom: 12 }}>
  {#snippet header({ page })}
    {#if page === 1}<h1>Invoice 1042</h1><p>Acme AS · Storgata 1 · 0155 Oslo</p>{:else}<p>Invoice 1042</p>{/if}
  {/snippet}
  {#snippet footer({ page, pages })}
    <p class="foot">Acme AS · Org. 912 345 678 <span>Page {page} of {pages}</span></p>
  {/snippet}

  <p>Ada Lovelace · Analytical Engines Ltd</p>
  <!-- The total is the table's foot: it goes with the last rows, on the last page. -->
  <Table
    rows={lines}
    columns={[
      "item",
      { key: "qty", align: "end", total: "sum" },
      { key: "price", align: "end", format: kr },
      { key: "amount", align: "end", format: kr, total: "sum" },
    ]}
    label="Lines"
    perPage={100}
  />
</Report>

<style>
  .foot { display: flex; justify-content: space-between; color: var(--elyra-muted); font-size: 12px; }
</style>
