<!--
  A form that knows Laravel's answers: `onsubmit` runs the save, and a
  validation failure (a `422`, or `ValidationErrors` from a command) puts
  each message under the `<Field name="…">` it belongs to. Messages for
  fields the form doesn't show, and any other failure, appear at the top.
  While it runs, the form is busy and its submit button says so.
-->
<script>
  import { setContext } from "svelte";
  import { validationErrors } from "@elyra/runtime";
  import { FORM } from "./internal/context.js";

  let { onsubmit = undefined, errors = $bindable({}), class: className = "", children, ...rest } = $props();

  let busy = $state(false);
  let failure = $state(null);
  /** The names of the fields in the form. */
  const fields = new Set();

  setContext(FORM, {
    get errors() {
      return errors;
    },
    get busy() {
      return busy;
    },
    register(name) {
      fields.add(name);
      return () => fields.delete(name);
    },
  });

  async function submit(event) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    failure = null;
    errors = {};
    try {
      await onsubmit?.(event);
    } catch (err) {
      const bag = validationErrors(err);
      if (bag) {
        errors = bag;
        const elsewhere = Object.entries(bag).filter(([name]) => !fields.has(name));
        if (elsewhere.length) failure = elsewhere.flatMap(([, messages]) => messages).join("\n");
      } else {
        failure = err?.detail ?? err?.message ?? String(err);
      }
    } finally {
      busy = false;
    }
  }
</script>

<form class="elyra-form {className}" novalidate aria-busy={busy || undefined} onsubmit={submit} {...rest}>
  {#if failure}<div class="failure" role="alert">{failure}</div>{/if}
  {@render children?.()}
</form>

<style>
  .elyra-form { display: grid; gap: calc(var(--elyra-space) * 2); }
  .failure {
    padding: 10px 12px; border-radius: var(--elyra-radius); white-space: pre-line;
    background: color-mix(in srgb, var(--elyra-danger) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--elyra-danger) 40%, transparent);
    color: var(--elyra-danger); font: 13px/1.45 var(--elyra-font);
  }
</style>
