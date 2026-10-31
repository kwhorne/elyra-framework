<!--
  A file to send with the form (RFC 0009): a button that opens the native
  file dialog, the chosen file's name (or the record's, `current`), and a
  way back. With `acceptsDrop`, a file dropped on the window while it's
  shown is taken too. `file` is a `ChosenFile`, what a command's file
  argument takes.
-->
<script>
  import { getContext } from "svelte";
  import { dialog, onFileDrop } from "@elyra/runtime";
  import Button from "./Button.svelte";
  import { FIELD } from "./internal/context.js";

  let {
    file = $bindable(null),
    current = null,
    filters = [],
    acceptsDrop = false,
    disabled = false,
    onchange = undefined,
    class: className = "",
    ...rest
  } = $props();

  const field = getContext(FIELD);
  const own = $props.id();
  const nameId = `${own}-name`;

  let failure = $state(null);
  let picking = $state(false);

  function take(chosen) {
    file = chosen;
    failure = null;
    onchange?.(chosen);
  }

  async function choose() {
    picking = true;
    try {
      const [picked] = await dialog.open({ filters });
      if (picked) take(picked);
    } catch (err) {
      failure = err?.detail ?? err?.message ?? String(err);
    } finally {
      picking = false;
    }
  }

  $effect(() => {
    if (!acceptsDrop || disabled) return;
    return onFileDrop((files) => {
      const dropped = files.find((f) => f.kind === "open");
      if (dropped) take(dropped);
    });
  });

  const describedBy = $derived([nameId, field?.describedBy].filter(Boolean).join(" "));
</script>

<div class="elyra-file-field {className}" class:invalid={field?.invalid}>
  <Button
    class="choose"
    id={field?.id}
    aria-describedby={describedBy}
    aria-invalid={field?.invalid || undefined}
    {disabled}
    loading={picking}
    onclick={choose}
    {...rest}
  >
    {file || current ? "Replace…" : "Choose a file…"}
  </Button>
  <span id={nameId} class="name" class:none={!file && !current}>
    {#if file}{file.name}{:else if current}{current}{:else}No file{/if}
  </span>
  {#if file && !disabled}
    <Button variant="ghost" size="sm" onclick={() => take(null)}>
      {current ? "Keep the current one" : "Remove"}
    </Button>
  {/if}
  {#if acceptsDrop && !file && !disabled}<span class="hint">or drop one on the window</span>{/if}
  {#if failure}<span class="failure" role="alert">{failure}</span>{/if}
</div>

<style>
  .elyra-file-field { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; font: 13px var(--elyra-font); }
  .invalid :global(.choose) { border-color: var(--elyra-danger); }
  .name { color: var(--elyra-text); overflow-wrap: anywhere; min-width: 0; }
  .name.none, .hint { color: var(--elyra-muted); }
  .hint { font-size: 12px; }
  .failure { flex-basis: 100%; color: var(--elyra-danger); font-size: 12px; }
</style>
