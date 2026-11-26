<script>
  import { Stat } from "@elyra/ui";

  const kroner = (v) => new Intl.NumberFormat("nb-NO", { style: "currency", currency: "NOK", maximumFractionDigits: 0 }).format(v);
</script>

<div class="kpis">
  <Stat label="Revenue" value={482000} format={kroner} change={0.12} caption="vs. last month" values={[310, 340, 330, 390, 420, 482]} />
  <Stat label="Open tickets" value={14} change={-0.25} good="down" caption="vs. last week" values={[22, 19, 20, 17, 14]} />
  <Stat label="Customers" value={1284} change={0} caption="vs. yesterday" />
</div>

<style>
  .kpis { display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; width: 100%; }
</style>
