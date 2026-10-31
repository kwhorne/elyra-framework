<!-- A panel that slides in from a side of the window: a `<Modal>` placed `side`. -->
<script>
  import Modal from "./Modal.svelte";

  let { open = $bindable(false), side = "right", children, footer = undefined, ...rest } = $props();
</script>

<Modal bind:open placement={side} {footer} {...rest}>
  {@render children?.()}
</Modal>
