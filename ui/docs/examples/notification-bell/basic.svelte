<script>
  import { NotificationBell, Toolbar } from "@elyra/ui";
</script>

<!-- Two unread: an invoice paid, and a mention. A click on one opens its page in the app. -->
<div class="window">
  <Toolbar title="CRM">
    {#snippet actions()}
      <NotificationBell href="/notifications" />
    {/snippet}
  </Toolbar>
  <div class="page"></div>
</div>

<style>
  .window { width: 100%; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); }
  .page { height: 360px; }
</style>
