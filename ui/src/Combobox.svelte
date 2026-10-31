<!--
  Type to find one of `options`: an input that filters a list as you type.
  `options` is a list (strings, or objects), or a function that searches:
  `(query, { signal }) => Promise<list | Page>`, so a resource's `_index`
  command can be the source. ↓ opens, ↓/↑ move, Enter picks, Esc closes,
  and leaving the field puts back what was chosen (or clears it, if the
  text was emptied).
-->
<script>
  import { getContext, tick, untrack } from "svelte";
  import Icon from "./internal/Icon.svelte";
  import Spinner from "./Spinner.svelte";
  import { FIELD } from "./internal/context.js";
  import { onOutside } from "./internal/focus.js";

  let {
    value = $bindable(null),
    label = $bindable(""),
    options = [],
    optionValue = "value",
    optionLabel = "label",
    placeholder = "Search…",
    minChars = 0,
    debounce = 200,
    emptyText = "No matches",
    clearable = true,
    disabled = false,
    onchange = undefined,
    class: className = "",
    ...rest
  } = $props();

  const field = getContext(FIELD);
  const own = $props.id();
  const listId = `${own}-list`;
  const optionId = (i) => `${own}-option-${i}`;

  const read = (key, o) => (typeof key === "function" ? key(o) : o?.[key]);
  /** An option as the list shows it, keeping the original for `onchange`. */
  function item(o) {
    if (o === null || typeof o !== "object") return { value: o, label: String(o), disabled: false, option: o };
    return { value: read(optionValue, o), label: String(read(optionLabel, o) ?? ""), disabled: !!o.disabled, option: o };
  }

  const remote = $derived(typeof options === "function");
  const local = $derived(remote ? [] : options.map(item));
  /** The chosen option's text: from the list when it has the value, else `label`. */
  const chosen = $derived(
    value === null || value === undefined ? "" : (local.find((o) => o.value === value)?.label ?? label ?? ""),
  );

  let text = $state("");
  let typed = $state(false);
  let open = $state(false);
  let active = $state(-1);
  let found = $state([]);
  let loading = $state(false);
  let failure = $state(null);
  let root = $state();
  let input = $state();

  const query = $derived(text.trim());
  const shown = $derived(
    remote
      ? found
      : typed && query
        ? local.filter((o) => o.label.toLowerCase().includes(query.toLowerCase()))
        : local,
  );
  const tooShort = $derived(remote && query.length < minChars);

  // While nothing is being typed, the input shows what's chosen.
  $effect(() => {
    const now = chosen;
    if (!untrack(() => typed)) text = now;
  });

  $effect(() => {
    if (open) return onOutside(() => [root], () => leave());
  });

  // A new list starts at the chosen option (when nothing was typed) or its first.
  $effect(() => {
    const list = shown;
    if (!open) return;
    const at = untrack(() => (typed ? -1 : list.findIndex((o) => o.value === value && !o.disabled)));
    active = at >= 0 ? at : next(list, -1, 1);
  });

  /** The next option that isn't disabled, from `from` in `step`'s direction, or -1. */
  function next(list, from, step) {
    for (let i = from + step; i >= 0 && i < list.length; i += step) {
      if (!list[i].disabled) return i;
    }
    return -1;
  }

  let seq = 0;
  let timer;
  let controller;

  /** Ask `options` for `q`, after `wait` ms; only the latest answer counts. */
  function search(q, wait) {
    clearTimeout(timer);
    controller?.abort();
    const n = ++seq;
    failure = null;
    if (q.length < minChars) {
      found = [];
      loading = false;
      return;
    }
    loading = true;
    timer = setTimeout(async () => {
      controller = new AbortController();
      try {
        const result = await options(q, { signal: controller.signal });
        if (n !== seq) return;
        found = (Array.isArray(result) ? result : (result?.data ?? [])).map(item);
      } catch (err) {
        if (n !== seq) return;
        found = [];
        failure = err?.detail ?? err?.message ?? String(err);
      } finally {
        if (n === seq) loading = false;
      }
    }, wait);
  }

  $effect(() => () => {
    clearTimeout(timer);
    controller?.abort();
  });

  function show() {
    if (disabled || open) return;
    open = true;
    if (remote) search(typed ? query : "", 0);
  }

  function close() {
    open = false;
    clearTimeout(timer);
    controller?.abort();
    seq++;
    loading = false;
  }

  function choose(o) {
    if (!o || o.disabled) return;
    typed = false;
    value = o.value;
    label = o.label;
    text = o.label;
    close();
    onchange?.(o.value, o.option);
  }

  function clear() {
    typed = false;
    value = null;
    label = "";
    text = "";
    close();
    onchange?.(null, null);
  }

  /** Focus left: an emptied field clears the choice, anything else goes back to it. */
  function leave() {
    close();
    if (typed && !query && value !== null && value !== undefined) return clear();
    typed = false;
    text = chosen;
  }

  async function scrollToActive() {
    await tick();
    root?.querySelector(`#${CSS.escape(optionId(active))}`)?.scrollIntoView?.({ block: "nearest" });
  }

  function oninput() {
    typed = true;
    if (!open) open = true;
    if (remote) search(query, debounce);
  }

  function onkeydown(event) {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        if (!open) return show();
        active = next(shown, active, 1) < 0 ? active : next(shown, active, 1);
        scrollToActive();
        break;
      case "ArrowUp":
        event.preventDefault();
        if (!open) return show();
        active = next(shown, active, -1) < 0 ? active : next(shown, active, -1);
        scrollToActive();
        break;
      case "Enter":
        if (!open) return;
        // An open list's Enter is the list's, not the form's.
        event.preventDefault();
        choose(shown[active]);
        break;
      case "Escape":
        if (!open) return;
        event.preventDefault();
        event.stopPropagation();
        close();
        typed = false;
        text = chosen;
        break;
    }
  }

  // Nothing else in the combobox takes focus, so focus leaving the input leaves it.
  function onfocusout() {
    if (open || typed) leave();
  }

  const hasValue = $derived(value !== null && value !== undefined);
  const popup = $derived(open && !tooShort);
</script>

<div class="elyra-combobox {className}" class:disabled bind:this={root}>
  <div class="box">
    <Icon name="search" />
    <input
      bind:this={input}
      bind:value={text}
      type="text"
      role="combobox"
      autocomplete="off"
      spellcheck="false"
      id={field?.id}
      required={field?.required || undefined}
      aria-autocomplete="list"
      aria-expanded={popup}
      aria-controls={listId}
      aria-activedescendant={popup && active >= 0 ? optionId(active) : undefined}
      aria-invalid={field?.invalid || undefined}
      aria-describedby={field?.describedBy}
      {placeholder}
      {disabled}
      {oninput}
      {onkeydown}
      onclick={show}
      {onfocusout}
      {...rest}
    />
    {#if loading}
      <Spinner size="sm" label="Searching" />
    {:else if clearable && hasValue && !disabled}
      <!-- Out of the tab order, and a press keeps focus in the input: the
           keyboard clears by emptying the text. -->
      <button
        type="button"
        class="clear"
        tabindex="-1"
        aria-label="Clear"
        onmousedown={(e) => e.preventDefault()}
        onclick={clear}
      >
        <Icon name="close" size={12} />
      </button>
    {/if}
  </div>

  {#if popup}
    <!-- Focus stays in the input: a press on the list mustn't take it. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="popup" onmousedown={(e) => e.preventDefault()}>
      {#if shown.length}
      <ul id={listId} role="listbox" aria-labelledby={field?.labelId} aria-busy={loading || undefined}>
        {#each shown as o, i (i)}
          <!-- The keys are the input's: ↓/↑, Enter, Esc. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            id={optionId(i)}
            role="option"
            aria-selected={hasValue && o.value === value}
            aria-disabled={o.disabled || undefined}
            class:active={i === active}
            onclick={() => choose(o)}
            onmousemove={() => !o.disabled && (active = i)}
          >
            <span>{o.label}</span>
            {#if hasValue && o.value === value}<Icon name="check" />{/if}
          </li>
        {/each}
      </ul>
      {/if}
      {#if failure}
        <p class="note failure" role="alert">{failure}</p>
      {:else if loading && shown.length === 0}
        <p class="note">Searching…</p>
      {:else if !loading && shown.length === 0}
        <p class="note">{emptyText}</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .elyra-combobox { position: relative; font: 13px var(--elyra-font); }
  .box {
    display: flex; align-items: center; gap: 6px;
    height: 32px; padding: 0 8px 0 10px; box-sizing: border-box;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    background: var(--elyra-inset); color: var(--elyra-muted);
  }
  .box:focus-within { border-color: var(--elyra-focus); box-shadow: 0 0 0 3px color-mix(in srgb, var(--elyra-focus) 25%, transparent); }
  .box:has(input[aria-invalid="true"]) { border-color: var(--elyra-danger); }
  .disabled .box { opacity: .55; }
  input {
    flex: 1; min-width: 0; height: 100%; padding: 0; border: 0; outline: none;
    background: transparent; color: var(--elyra-text); font: inherit;
  }
  input::placeholder { color: var(--elyra-muted); }
  .clear {
    display: grid; place-items: center; width: 20px; height: 20px; padding: 0;
    border: 0; border-radius: calc(var(--elyra-radius) - 2px);
    background: transparent; color: var(--elyra-muted); cursor: pointer;
  }
  .clear:hover { background: var(--elyra-accent-soft); color: var(--elyra-text); }
  .clear:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  .popup {
    position: absolute; z-index: 50; left: 0; right: 0; top: calc(100% + 4px);
    background: var(--elyra-raised); border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    box-shadow: var(--elyra-shadow); overflow: hidden;
  }
  ul { list-style: none; margin: 0; padding: 4px; max-height: 240px; overflow: auto; }
  li {
    display: flex; align-items: center; justify-content: space-between; gap: 8px;
    padding: 6px 8px; border-radius: calc(var(--elyra-radius) - 2px); color: var(--elyra-text); cursor: pointer;
  }
  li.active { background: var(--elyra-accent-soft); }
  li[aria-selected="true"] { color: var(--elyra-accent); font-weight: 500; }
  li[aria-disabled="true"] { opacity: .45; cursor: default; }
  .note { margin: 0; padding: 8px 12px; color: var(--elyra-muted); }
  .note.failure { color: var(--elyra-danger); }
</style>
