<script>
  import { Checkbox, Field, Popover, Select } from "@elyra/ui";

  let status = $state("Active");
  let vip = $state(false);
</script>

<Popover label="Filter">
  <Field label="Status"><Select bind:value={status} options={["Active", "Paused", "Closed"]} /></Field>
  <Checkbox bind:checked={vip}>Only VIP customers</Checkbox>
</Popover>
<span>{status}{vip ? ", VIP" : ""}</span>
