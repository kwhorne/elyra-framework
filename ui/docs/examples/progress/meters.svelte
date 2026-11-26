<script>
  import { Progress } from "@elyra/ui";

  const gb = (v, max) => `${v} of ${max} GB`;
</script>

<div class="stack">
  <Progress kind="meter" label="Storage" value={42} max={50} low={35} high={45} format={gb} showValue />
  <Progress kind="meter" label="Seats" value={18} max={25} low={20} high={24} showValue />
  <Progress kind="meter" label="Battery" value={9} low={30} high={10} showValue />
</div>

<style>
  .stack { display: grid; gap: 16px; width: 100%; max-width: 360px; }
</style>
