<script>
  import { Callout } from "@elyra/ui";
</script>

<div class="stack">
  <Callout title="Syncs every 5 minutes">Changes made on the web show up here on their own.</Callout>
  <Callout variant="success" title="Paid on time">INV-2033 was paid on 2 October.</Callout>
  <Callout variant="warning" title="Due in 3 days">INV-2050, 420 €.</Callout>
  <Callout variant="danger" title="2 invoices overdue">INV-2041 and INV-2047, 1 860 € in all.</Callout>
</div>

<style>
  .stack { display: grid; gap: 10px; width: 100%; }
</style>
