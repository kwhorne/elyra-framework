<!--
  One of a few choices: native radios (arrow keys move between them), drawn
  to match. `options` are strings or `{ value, label, disabled }`.
-->
<script>
  import { getContext } from "svelte";
  import { FIELD } from "./internal/context.js";

  let { value = $bindable(undefined), options = [], label = undefined, inline = false, class: className = "", ...rest } = $props();
  const field = getContext(FIELD);
  const own = $props.id();
  const items = $derived(options.map((o) => (typeof o === "object" ? o : { value: o, label: String(o) })));
</script>

<div
  role="radiogroup"
  aria-label={label}
  aria-labelledby={label ? undefined : field?.labelId}
  aria-describedby={field?.describedBy}
  aria-invalid={field?.invalid || undefined}
  class="elyra-radio-group {className}"
  class:inline
  {...rest}
>
  {#each items as item (item.value)}
    <label class="option" class:disabled={item.disabled}>
      <input type="radio" name={field?.name ?? own} value={item.value} bind:group={value} disabled={item.disabled} />
      <span class="dot" aria-hidden="true"></span>
      <span>{item.label}</span>
    </label>
  {/each}
</div>

<style>
  .elyra-radio-group { display: grid; gap: 8px; font: 13px var(--elyra-font); color: var(--elyra-text); }
  .elyra-radio-group.inline { display: flex; flex-wrap: wrap; gap: 16px; }
  .option { display: inline-flex; align-items: center; gap: 8px; cursor: pointer; }
  .option.disabled { opacity: .55; cursor: default; }
  input { position: absolute; opacity: 0; width: 1px; height: 1px; margin: 0; }
  .dot {
    width: 16px; height: 16px; flex: none; box-sizing: border-box; border-radius: 50%;
    border: 1px solid var(--elyra-border); background: var(--elyra-inset);
    transition: border-color .12s, border-width .12s;
  }
  input:checked + .dot { border: 5px solid var(--elyra-accent); background: var(--elyra-on-accent); }
  input:focus-visible + .dot { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
</style>
