<script>
  import { Card, Presence } from "@elyra/ui";

  const id = 42;
</script>

<!-- Ada and Linus have customer 42 open in the browser; the reader isn't shown. -->
<Card title="Ada Lovelace AS">
  {#snippet actions()}<Presence channel={`customers.${id}`} />{/snippet}
  <p class="note">Credit limit 50 000 kr · last order 31 October</p>
</Card>

<style>
  .note { margin: 0; color: var(--elyra-muted); }
</style>
