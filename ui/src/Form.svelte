<!--
  A form that knows Laravel's answers: `onsubmit` runs the save, and a
  validation failure (a `422`, or `ValidationErrors` from a command) puts
  each message under the `<Field name="…">` it belongs to. Messages for
  fields the form doesn't show, and any other failure, appear at the top.
  The first field with a message takes focus. While it runs, the form is
  busy and its submit button says so.

  `bind:dirty` is true once the user changed something, and false again
  after a save went through: ask before a Modal with unsaved changes
  closes, or tell `<Presence editing>`.
-->
<script>
  import { setContext, tick } from "svelte";
  import { validationErrors } from "@elyra/runtime";
  import { FORM } from "./internal/context.js";

  let {
    onsubmit = undefined,
    errors = $bindable({}),
    dirty = $bindable(false),
    oninput = undefined,
    onchange = undefined,
    class: className = "",
    children,
    ...rest
  } = $props();

  let busy = $state(false);
  let failure = $state(null);
  let form = $state();
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
    /** A control that isn't a plain input says the user changed it. */
    changed() {
      dirty = true;
    },
  });

  // These say so themselves, when a choice is made: what's typed in their
  // search box or into their text isn't a change yet.
  const OWN_CHANGES = ".elyra-select, .elyra-combobox, .elyra-datepicker, .elyra-file-field, .elyra-switch-wrap";

  /** A plain input, textarea, checkbox or radio the user changed. */
  function edited(event) {
    if (!event.target?.closest?.(OWN_CHANGES)) dirty = true;
    (event.type === "input" ? oninput : onchange)?.(event);
  }

  async function submit(event) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    failure = null;
    errors = {};
    try {
      await onsubmit?.(event);
      dirty = false;
    } catch (err) {
      const bag = validationErrors(err);
      if (bag) {
        errors = bag;
        const elsewhere = Object.entries(bag).filter(([name]) => !fields.has(name));
        if (elsewhere.length) failure = elsewhere.flatMap(([, messages]) => messages).join("\n");
        // The first field the user has to fix, in the form's order: its
        // control, which says `aria-invalid` itself unless it's a button
        // (a `<Microphone>`), which can't.
        await tick();
        form?.querySelector('[aria-invalid="true"], .elyra-field.invalid [id$="-control"]')?.focus();
      } else {
        failure = err?.detail ?? err?.message ?? String(err);
      }
    } finally {
      busy = false;
    }
  }
</script>

<form
  bind:this={form}
  class="elyra-form {className}"
  novalidate
  aria-busy={busy || undefined}
  onsubmit={submit}
  oninput={edited}
  onchange={edited}
  {...rest}
>
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
