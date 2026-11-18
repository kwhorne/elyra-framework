<script>
  import Modal from "../Modal.svelte";

  let { beforeclose, onclose = undefined, open = $bindable(true) } = $props();
</script>

<Modal bind:open title="Edit customer" {beforeclose} {onclose}>
  <input aria-label="Name" />
</Modal>
