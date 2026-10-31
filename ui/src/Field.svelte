<!--
  A label, a control, and its help or error. Inside a `<Form>`, `name` picks
  the field's message from the form's errors; `error` sets one directly. The
  control inside (`Input`, `Select`, …) gets the id, `aria-invalid` and
  `aria-describedby` on its own.
-->
<script>
  import { getContext, setContext } from "svelte";
  import { FIELD, FORM } from "./internal/context.js";

  let {
    name = undefined,
    label = undefined,
    description = undefined,
    error = undefined,
    required = false,
    class: className = "",
    children,
  } = $props();

  const id = $props.id();
  const form = getContext(FORM);

  $effect(() => {
    if (name && form) return form.register(name);
  });

  const message = $derived(error ?? (name ? form?.errors?.[name]?.[0] : undefined));
  const describedBy = $derived(
    message ? `${id}-error` : description ? `${id}-description` : undefined,
  );

  setContext(FIELD, {
    id: `${id}-control`,
    labelId: `${id}-label`,
    get name() {
      return name;
    },
    get invalid() {
      return !!message;
    },
    get describedBy() {
      return describedBy;
    },
    get required() {
      return required;
    },
  });
</script>

<div class="elyra-field {className}" class:invalid={!!message}>
  {#if label}
    <label id="{id}-label" for="{id}-control">
      {label}{#if required}<span class="required" aria-hidden="true">*</span>{/if}
    </label>
  {/if}
  {@render children?.()}
  {#if message}
    <small id="{id}-error" class="error">{message}</small>
  {:else if description}
    <small id="{id}-description" class="description">{description}</small>
  {/if}
</div>

<style>
  .elyra-field { display: grid; gap: 6px; font: var(--elyra-font-size)/1.4 var(--elyra-font); }
  label { color: var(--elyra-text); font-size: 13px; font-weight: 500; }
  .required { color: var(--elyra-danger); margin-left: 2px; }
  .description { color: var(--elyra-muted); font-size: 12px; }
  .error { color: var(--elyra-danger); font-size: 12px; }
</style>
