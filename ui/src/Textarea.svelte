<!-- A text area. Inside a `<Field>`, it takes the field's id and state. -->
<script>
  import { getContext } from "svelte";
  import { FIELD } from "./internal/context.js";

  let { value = $bindable(""), rows = 4, class: className = "", ...rest } = $props();
  const field = getContext(FIELD);
</script>

<textarea
  bind:value
  {rows}
  id={field?.id}
  name={field?.name}
  required={field?.required || undefined}
  aria-invalid={field?.invalid || undefined}
  aria-describedby={field?.describedBy}
  class="elyra-textarea {className}"
  {...rest}
></textarea>

<style>
  .elyra-textarea {
    width: 100%; box-sizing: border-box; padding: 8px 10px; resize: vertical;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    background: var(--elyra-inset); color: var(--elyra-text);
    font: 13px/1.5 var(--elyra-font);
  }
  .elyra-textarea::placeholder { color: var(--elyra-muted); }
  .elyra-textarea:focus { outline: none; border-color: var(--elyra-focus); box-shadow: 0 0 0 3px color-mix(in srgb, var(--elyra-focus) 25%, transparent); }
  .elyra-textarea[aria-invalid="true"] { border-color: var(--elyra-danger); }
</style>
