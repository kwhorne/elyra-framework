<!--
  Choose one of `options` (strings, or `{ value, label, disabled }`): a
  button that opens a list, with a search box when the list is long
  (`searchable`, on by default from 8 options). The keyboard does it all:
  ↓/↑ or Enter opens, ↓/↑/Home/End move, Enter picks, Esc closes, and typing
  jumps to (or, with search, filters by) what was typed.
-->
<script>
  import { getContext, tick } from "svelte";
  import Icon from "./internal/Icon.svelte";
  import { FIELD } from "./internal/context.js";
  import { onOutside } from "./internal/focus.js";

  let {
    value = $bindable(undefined),
    options = [],
    placeholder = "Choose…",
    searchable = undefined,
    disabled = false,
    onchange = undefined,
    class: className = "",
    ...rest
  } = $props();

  const field = getContext(FIELD);
  const own = $props.id();
  const listId = `${own}-list`;

  const items = $derived(options.map((o) => (typeof o === "object" ? o : { value: o, label: String(o) })));
  const search = $derived(searchable ?? items.length >= 8);
  const selected = $derived(items.find((o) => o.value === value));

  let open = $state(false);
  let query = $state("");
  let active = $state(-1);
  let root = $state();
  let trigger = $state();
  let list = $state();
  let searchBox = $state();
  let typed = "";
  let typedAt = 0;

  const shown = $derived(
    search && query.trim()
      ? items.filter((o) => o.label.toLowerCase().includes(query.trim().toLowerCase()))
      : items,
  );
  const optionId = (i) => `${own}-option-${i}`;

  $effect(() => {
    if (open) return onOutside(() => [root], () => close(false));
  });

  async function show() {
    if (disabled || open) return;
    open = true;
    query = "";
    active = Math.max(0, items.findIndex((o) => o.value === value));
    await tick();
    (search ? searchBox : list)?.focus();
    scrollToActive();
  }

  function close(refocus = true) {
    open = false;
    if (refocus) trigger?.focus();
  }

  function pick(item) {
    if (!item || item.disabled) return;
    value = item.value;
    onchange?.(item.value);
    close();
  }

  /** The next option that isn't disabled, from `from` in `step`'s direction. */
  function move(from, step) {
    for (let i = from + step; i >= 0 && i < shown.length; i += step) {
      if (!shown[i].disabled) return i;
    }
    return active;
  }

  async function scrollToActive() {
    await tick();
    list?.querySelector(`#${CSS.escape(optionId(active))}`)?.scrollIntoView?.({ block: "nearest" });
  }

  function onTriggerKey(event) {
    if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
      event.preventDefault();
      show();
    }
  }

  function onListKey(event) {
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        active = move(active, 1);
        scrollToActive();
        break;
      case "ArrowUp":
        event.preventDefault();
        active = move(active, -1);
        scrollToActive();
        break;
      case "Home":
        event.preventDefault();
        active = move(-1, 1);
        scrollToActive();
        break;
      case "End":
        event.preventDefault();
        active = move(shown.length, -1);
        scrollToActive();
        break;
      case "Enter":
        event.preventDefault();
        pick(shown[active]);
        break;
      case "Escape":
        event.preventDefault();
        close();
        break;
      case "Tab":
        close(false);
        break;
      default:
        // Without a search box, typing jumps to the option it starts.
        if (!search && event.key.length === 1) {
          const now = Date.now();
          typed = now - typedAt > 600 ? event.key : typed + event.key;
          typedAt = now;
          const at = shown.findIndex((o) => !o.disabled && o.label.toLowerCase().startsWith(typed.toLowerCase()));
          if (at >= 0) {
            active = at;
            scrollToActive();
          }
        }
    }
  }

  $effect(() => {
    // A new filter starts from its first match.
    query;
    if (search && open) active = move(-1, 1);
  });
</script>

<div class="elyra-select {className}" bind:this={root}>
  <button
    bind:this={trigger}
    type="button"
    role="combobox"
    id={field?.id}
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-controls={listId}
    aria-invalid={field?.invalid || undefined}
    aria-describedby={field?.describedBy}
    {disabled}
    class="trigger"
    class:placeholder={!selected}
    onclick={() => (open ? close() : show())}
    onkeydown={onTriggerKey}
    {...rest}
  >
    <span class="value">{selected?.label ?? placeholder}</span>
    <Icon name="chevron-down" />
  </button>

  {#if open}
    <div class="popup">
      {#if search}
        <div class="search">
          <Icon name="search" />
          <input
            bind:this={searchBox}
            bind:value={query}
            type="text"
            placeholder="Search…"
            aria-label="Search the options"
            aria-controls={listId}
            aria-activedescendant={active >= 0 ? optionId(active) : undefined}
            onkeydown={onListKey}
          />
        </div>
      {/if}
      <ul
        bind:this={list}
        id={listId}
        role="listbox"
        tabindex="-1"
        aria-labelledby={field?.labelId}
        aria-activedescendant={!search && active >= 0 ? optionId(active) : undefined}
        onkeydown={onListKey}
      >
        {#each shown as item, i (item.value)}
          <!-- The keys are the list's (and the search box's): ↓/↑, Enter, Esc. -->
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <li
            id={optionId(i)}
            role="option"
            aria-selected={item.value === value}
            aria-disabled={item.disabled || undefined}
            class:active={i === active}
            onclick={() => pick(item)}
            onmousemove={() => !item.disabled && (active = i)}
          >
            <span>{item.label}</span>
            {#if item.value === value}<Icon name="check" />{/if}
          </li>
        {:else}
          <li class="empty" role="presentation">No matches</li>
        {/each}
      </ul>
    </div>
  {/if}
</div>

<style>
  .elyra-select { position: relative; font: 13px var(--elyra-font); }
  .trigger {
    display: flex; align-items: center; justify-content: space-between; gap: 8px;
    width: 100%; height: 32px; padding: 0 10px; box-sizing: border-box;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    background: var(--elyra-inset); color: var(--elyra-text); font: inherit; text-align: left; cursor: pointer;
  }
  .trigger.placeholder .value { color: var(--elyra-muted); }
  .trigger:focus-visible, .trigger[aria-expanded="true"] { outline: none; border-color: var(--elyra-focus); box-shadow: 0 0 0 3px color-mix(in srgb, var(--elyra-focus) 25%, transparent); }
  .trigger[aria-invalid="true"] { border-color: var(--elyra-danger); }
  .trigger:disabled { opacity: .55; cursor: default; }
  .value { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .popup {
    position: absolute; z-index: 50; left: 0; right: 0; top: calc(100% + 4px);
    background: var(--elyra-raised); border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    box-shadow: var(--elyra-shadow); overflow: hidden;
  }
  .search { display: flex; align-items: center; gap: 6px; padding: 0 10px; border-bottom: 1px solid var(--elyra-border); color: var(--elyra-muted); }
  .search input { flex: 1; height: 32px; border: 0; background: transparent; color: var(--elyra-text); font: inherit; outline: none; }
  ul { list-style: none; margin: 0; padding: 4px; max-height: 240px; overflow: auto; outline: none; }
  li {
    display: flex; align-items: center; justify-content: space-between; gap: 8px;
    padding: 6px 8px; border-radius: calc(var(--elyra-radius) - 2px); color: var(--elyra-text); cursor: pointer;
  }
  li.active { background: var(--elyra-accent-soft); }
  li[aria-selected="true"] { color: var(--elyra-accent); font-weight: 500; }
  li[aria-disabled="true"] { opacity: .45; cursor: default; }
  li.empty { color: var(--elyra-muted); cursor: default; }
</style>
