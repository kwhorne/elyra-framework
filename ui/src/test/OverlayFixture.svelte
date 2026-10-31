<script>
  import Modal from "../Modal.svelte";
  import Drawer from "../Drawer.svelte";
  import Tooltip from "../Tooltip.svelte";
  import Popover from "../Popover.svelte";
  import Button from "../Button.svelte";

  let { open = $bindable(false), drawer = $bindable(false), dismissible = true, onclose = undefined } = $props();
</script>

<button type="button" onclick={() => (open = true)}>Open</button>

<Modal bind:open title="Delete customer" description="It can't be undone." {dismissible} {onclose}>
  <input aria-label="Reason" />
  {#snippet footer()}
    <Button onclick={() => (open = false)}>Cancel</Button>
    <Button variant="danger">Delete</Button>
  {/snippet}
</Modal>

<Drawer bind:open={drawer} title="Filters">Filters here</Drawer>

<Tooltip text="Copies the link"><button type="button">Copy</button></Tooltip>

<Popover label="Filter">
  <input aria-label="Search in filter" />
</Popover>
