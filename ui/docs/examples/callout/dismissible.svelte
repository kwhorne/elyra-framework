<script>
  import { Callout } from "@elyra/ui";

  let hidden = $state(false);
</script>

<Callout variant="info" title="New: the assistant" dismissible ondismiss={() => (hidden = true)}>
  Press the Assistant button in the status bar and ask about a customer.
</Callout>
{#if hidden}<p>Dismissed. Reload to see it again.</p>{/if}
