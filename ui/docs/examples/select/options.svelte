<script>
  import { Field, Select } from "@elyra/ui";

  let team = $state(undefined);
  const TEAMS = [
    { value: 1, label: "Sales" },
    { value: 2, label: "Support" },
    { value: 3, label: "Operations", disabled: true },
  ];
</script>

<Field label="Team" description={team ? `Team #${team}` : "Nobody's yet."}>
  <Select bind:value={team} options={TEAMS} placeholder="No team" />
</Field>
