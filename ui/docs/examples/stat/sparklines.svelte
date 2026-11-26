<script>
  import { Sparkline } from "@elyra/ui";

  const week = [12, 18, 9, 22, 30, 26, 34];
</script>

<div class="row">
  <Sparkline values={week} label="Orders this week" />
  <Sparkline values={week} kind="area" tone="success" />
  <Sparkline values={week} kind="bars" width={64} />
  <Sparkline values={[40, 38, 31, 27, 22]} tone="auto" />
</div>

<style>
  .row { display: flex; gap: 20px; align-items: center; }
</style>
