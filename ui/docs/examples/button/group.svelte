<script>
  import { Button, ButtonGroup } from "@elyra/ui";

  let view = $state("list");
</script>

<ButtonGroup label="View">
  {#each ["list", "board", "calendar"] as option (option)}
    <Button size="sm" variant={view === option ? "primary" : "default"} aria-pressed={view === option} onclick={() => (view = option)}>
      {option[0].toUpperCase() + option.slice(1)}
    </Button>
  {/each}
</ButtonGroup>
