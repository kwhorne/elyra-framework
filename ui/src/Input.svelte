<!-- A text input. Inside a `<Field>`, it takes the field's id and state. -->
<script>
  import { getContext } from "svelte";
  import { FIELD } from "./internal/context.js";

  let { value = $bindable(""), type = "text", class: className = "", ...rest } = $props();
  const field = getContext(FIELD);
</script>

<input
  {type}
  bind:value
  id={field?.id}
  name={field?.name}
  required={field?.required || undefined}
  aria-invalid={field?.invalid || undefined}
  aria-describedby={field?.describedBy}
  class="elyra-input {className}"
  {...rest}
/>

<style>
  .elyra-input {
    width: 100%; height: 32px; box-sizing: border-box; padding: 0 10px;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    background: var(--elyra-inset); color: var(--elyra-text);
    font: 13px var(--elyra-font);
  }
  .elyra-input::placeholder { color: var(--elyra-muted); }
  .elyra-input:focus { outline: none; border-color: var(--elyra-focus); box-shadow: 0 0 0 3px color-mix(in srgb, var(--elyra-focus) 25%, transparent); }
  .elyra-input[aria-invalid="true"] { border-color: var(--elyra-danger); }
  .elyra-input:disabled { opacity: .55; }
</style>
