<script>
  import { Button, Drawer, Field, Select, Switch } from "@elyra/ui";

  let open = $state(false);
  let country = $state(undefined);
  let active = $state(true);
</script>

<Button onclick={() => (open = true)}>Filters</Button>

<Drawer bind:open title="Filters">
  <Field label="Country"><Select bind:value={country} options={["Denmark", "Norway", "Sweden"]} placeholder="Any" /></Field>
  <Switch bind:checked={active}>Only active customers</Switch>
  {#snippet footer()}
    <Button variant="primary" onclick={() => (open = false)}>Apply</Button>
  {/snippet}
</Drawer>
