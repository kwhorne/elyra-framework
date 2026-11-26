<script>
  import { Progress } from "@elyra/ui";
</script>

<div class="stack">
  <Progress label="Uploading invoices.pdf" value={62} showValue />
  <Progress label="Syncing" />
  <Progress label="Step 3 of 5" value={3} max={5} size="sm" />
</div>

<style>
  .stack { display: grid; gap: 16px; width: 100%; max-width: 360px; }
</style>
