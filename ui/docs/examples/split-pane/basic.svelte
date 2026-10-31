<script>
  import { Badge, SplitPane } from "@elyra/ui";

  const customers = [
    { id: 1, name: "Ada Lovelace", city: "London", open: 2 },
    { id: 2, name: "Grace Hopper", city: "Arlington", open: 0 },
    { id: 3, name: "Kristen Nygaard", city: "Oslo", open: 1 },
  ];
  let chosen = $state(customers[0]);
</script>

<!-- The pane fills what it's in; here it's a box. -->
<div class="window">
  <SplitPane size={200} min={140} max={320} persist="docs-customers">
    {#snippet first()}
      <ul class="list">
        {#each customers as c (c.id)}
          <li><button class:active={c === chosen} onclick={() => (chosen = c)}>{c.name}</button></li>
        {/each}
      </ul>
    {/snippet}
    {#snippet second()}
      <article class="record">
        <h3>{chosen.name}</h3>
        <p>{chosen.city}</p>
        <Badge variant={chosen.open ? "warning" : "success"}>{chosen.open} open invoices</Badge>
      </article>
    {/snippet}
  </SplitPane>
</div>

<style>
  .window { width: 100%; height: 220px; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden; }
  .list { list-style: none; margin: 0; padding: 6px; }
  .list button {
    width: 100%; padding: 6px 8px; border: 0; border-radius: var(--elyra-radius);
    background: transparent; color: var(--elyra-text); font: inherit; text-align: left; cursor: pointer;
  }
  .list button.active { background: var(--elyra-accent-soft); }
  .record { padding: 16px; }
  .record h3 { margin: 0 0 4px; }
  .record p { margin: 0 0 12px; color: var(--elyra-muted); }
</style>
