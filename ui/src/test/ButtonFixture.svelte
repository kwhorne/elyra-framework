<script>
  import Button from "../Button.svelte";
  let { label = "Save", ...props } = $props();
</script>

<Button {...props}>{label}</Button>
