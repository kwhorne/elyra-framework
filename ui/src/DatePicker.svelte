<!--
  A date: typed, or picked in a calendar. The value is an ISO date
  (`2026-11-30`) or null, whatever the locale; the field shows it as the
  locale writes dates in numbers, and takes that or ISO when typed. The
  calendar is a dialog with a grid: arrows move a day or a week, Home/End
  to the week's ends, PageUp/PageDown a month (Shift: a year), Enter
  picks, Esc closes. `min` and `max` bound it.
-->
<script>
  import { getContext, tick, untrack } from "svelte";
  import Icon from "./internal/Icon.svelte";
  import { FIELD } from "./internal/context.js";
  import { onOutside, trapTab } from "./internal/focus.js";
  import {
    addDays,
    addMonths,
    clamp,
    formatNumeric,
    localeWeekStart,
    monthGrid,
    numericPattern,
    parseTyped,
    parts,
    startOfWeek,
    today as todayIso,
  } from "./internal/dates.js";

  let {
    value = $bindable(null),
    min = undefined,
    max = undefined,
    locale = undefined,
    weekStart = undefined,
    placeholder = undefined,
    clearable = true,
    disabled = false,
    onchange = undefined,
    class: className = "",
    ...rest
  } = $props();

  const field = getContext(FIELD);
  const own = $props.id();
  const titleId = `${own}-title`;

  const start = $derived(weekStart ?? localeWeekStart(locale));
  const hint = $derived(placeholder ?? numericPattern(locale));
  const valid = $derived(parts(value) ? value : null);
  const today = todayIso();

  const long = (day) =>
    new Intl.DateTimeFormat(locale, { dateStyle: "full", timeZone: "UTC" }).format(new Date(`${day}T00:00:00Z`));
  const monthTitle = (day) =>
    new Intl.DateTimeFormat(locale, { month: "long", year: "numeric", timeZone: "UTC" }).format(
      new Date(`${day}T00:00:00Z`),
    );
  const weekdays = $derived.by(() => {
    const first = startOfWeek("2026-11-04", start);
    const short = new Intl.DateTimeFormat(locale, { weekday: "short", timeZone: "UTC" });
    const full = new Intl.DateTimeFormat(locale, { weekday: "long", timeZone: "UTC" });
    return Array.from({ length: 7 }, (_, i) => {
      const date = new Date(`${addDays(first, i)}T00:00:00Z`);
      return { short: short.format(date), long: full.format(date) };
    });
  });

  let text = $state("");
  let typed = $state(false);
  let open = $state(false);
  /** The day the calendar's focus is on. */
  let focus = $state(today);
  let root = $state();
  let dialog = $state();
  let opener = $state();

  const grid = $derived(monthGrid(focus, start));
  const month = $derived(focus.slice(0, 7));
  const outOfRange = (day) => (min && day < min) || (max && day > max);

  // While nothing is being typed, the field shows the value.
  $effect(() => {
    const now = formatNumeric(valid, locale);
    if (!untrack(() => typed)) text = now;
  });

  $effect(() => {
    if (open) return onOutside(() => [root], () => close(false));
  });

  function set(day) {
    typed = false;
    text = formatNumeric(day, locale);
    if (day === value) return;
    value = day;
    onchange?.(day);
  }

  async function show() {
    if (disabled || open) return;
    focus = clamp(valid ?? today, min, max);
    open = true;
    await tick();
    focusDay();
  }

  function close(back = true) {
    open = false;
    if (back) opener?.focus();
  }

  async function focusDay() {
    await tick();
    dialog?.querySelector(`[data-date="${focus}"]`)?.focus();
  }

  function pick(day) {
    if (outOfRange(day)) return;
    set(day);
    close();
  }

  function move(to) {
    focus = to;
    focusDay();
  }

  function ongridkey(event) {
    const keys = {
      ArrowLeft: () => addDays(focus, -1),
      ArrowRight: () => addDays(focus, 1),
      ArrowUp: () => addDays(focus, -7),
      ArrowDown: () => addDays(focus, 7),
      Home: () => startOfWeek(focus, start),
      End: () => addDays(startOfWeek(focus, start), 6),
      PageUp: () => addMonths(focus, event.shiftKey ? -12 : -1),
      PageDown: () => addMonths(focus, event.shiftKey ? 12 : 1),
    };
    if (keys[event.key]) {
      event.preventDefault();
      move(keys[event.key]());
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      pick(focus);
    }
  }

  function ondialogkey(event) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      close();
      return;
    }
    trapTab(event, dialog);
  }

  /** The typed text, taken: a date in range, empty for none, or back to the value. */
  function commit() {
    if (!typed) return;
    const t = text.trim();
    if (!t) {
      typed = false;
      if (value !== null) {
        value = null;
        onchange?.(null);
      }
      return;
    }
    const day = parseTyped(t, locale);
    if (day && !outOfRange(day)) return set(day);
    typed = false;
    text = formatNumeric(valid, locale);
  }

  function clear() {
    typed = false;
    text = "";
    if (value === null) return;
    value = null;
    onchange?.(null);
  }

  function oninput() {
    typed = true;
  }

  function oninputkey(event) {
    if (event.key === "Enter" && typed) {
      // A typed date is the field's, not the form's: take it first.
      commit();
    } else if (event.key === "ArrowDown" && (event.altKey || !typed)) {
      event.preventDefault();
      commit();
      show();
    }
  }
</script>

<div class="elyra-datepicker {className}" class:disabled bind:this={root}>
  <div class="box">
    <input
      bind:value={text}
      type="text"
      inputmode="numeric"
      autocomplete="off"
      spellcheck="false"
      id={field?.id}
      required={field?.required || undefined}
      aria-invalid={field?.invalid || undefined}
      aria-describedby={field?.describedBy}
      placeholder={hint}
      {disabled}
      {oninput}
      onkeydown={oninputkey}
      onblur={commit}
      {...rest}
    />
    {#if clearable && valid && !disabled}
      <button
        type="button"
        class="icon clear"
        tabindex="-1"
        aria-label="Clear"
        onmousedown={(e) => e.preventDefault()}
        onclick={clear}
      >
        <Icon name="close" size={12} />
      </button>
    {/if}
    <button
      bind:this={opener}
      type="button"
      class="icon open"
      aria-label={valid ? `Choose date, ${long(valid)}` : "Choose date"}
      aria-haspopup="dialog"
      aria-expanded={open}
      {disabled}
      onclick={() => (open ? close() : show())}
    >
      <Icon name="calendar" />
    </button>
  </div>

  {#if open}
    <div class="popup" role="dialog" aria-modal="true" aria-labelledby={titleId} bind:this={dialog} tabindex="-1" onkeydown={ondialogkey}>
      <div class="head">
        <button type="button" class="icon" aria-label="Previous month" onclick={() => move(addMonths(focus, -1))}>
          <Icon name="chevron-left" />
        </button>
        <span id={titleId} class="title" aria-live="polite">{monthTitle(focus)}</span>
        <button type="button" class="icon" aria-label="Next month" onclick={() => move(addMonths(focus, 1))}>
          <Icon name="chevron-right" />
        </button>
      </div>
      <table role="grid" aria-labelledby={titleId}>
        <thead>
          <tr>
            {#each weekdays as w (w.long)}
              <th scope="col" abbr={w.long}>{w.short}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each grid as week (week[0])}
            <tr>
              {#each week as day (day)}
                <!-- The grid's keys are on each day: one is in the tab order at a time. -->
                <td
                  role="gridcell"
                  data-date={day}
                  tabindex={day === focus ? 0 : -1}
                  aria-label={long(day)}
                  aria-selected={day === valid}
                  aria-disabled={outOfRange(day) || undefined}
                  aria-current={day === today ? "date" : undefined}
                  class:outside={day.slice(0, 7) !== month}
                  onclick={() => pick(day)}
                  onkeydown={ongridkey}
                >
                  {Number(day.slice(8))}
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
      <div class="foot">
        <button type="button" class="text" disabled={outOfRange(today)} onclick={() => pick(today)}>Today</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .elyra-datepicker { position: relative; font: 13px var(--elyra-font); }
  .box {
    display: flex; align-items: center; gap: 4px;
    height: 32px; padding: 0 4px 0 10px; box-sizing: border-box;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    background: var(--elyra-inset); color: var(--elyra-muted);
  }
  .box:focus-within { border-color: var(--elyra-focus); box-shadow: 0 0 0 3px color-mix(in srgb, var(--elyra-focus) 25%, transparent); }
  .box:has(input[aria-invalid="true"]) { border-color: var(--elyra-danger); }
  .disabled .box { opacity: .55; }
  input {
    flex: 1; min-width: 0; height: 100%; padding: 0; border: 0; outline: none;
    background: transparent; color: var(--elyra-text); font: inherit; font-variant-numeric: tabular-nums;
  }
  input::placeholder { color: var(--elyra-muted); }
  .icon {
    display: grid; place-items: center; width: 24px; height: 24px; padding: 0;
    border: 0; border-radius: calc(var(--elyra-radius) - 2px);
    background: transparent; color: var(--elyra-muted); cursor: pointer;
  }
  .icon:hover:not(:disabled) { background: var(--elyra-accent-soft); color: var(--elyra-text); }
  .icon:focus-visible, .text:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  .icon:disabled { cursor: default; }
  .clear { width: 20px; height: 20px; }
  .popup {
    position: absolute; z-index: 50; left: 0; top: calc(100% + 4px); width: 264px; padding: 8px;
    box-sizing: border-box; outline: none;
    background: var(--elyra-raised); border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    box-shadow: var(--elyra-shadow);
  }
  .head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px; }
  .title { font-weight: 600; color: var(--elyra-text); text-transform: capitalize; }
  table { width: 100%; border-collapse: collapse; table-layout: fixed; }
  th { padding: 4px 0; font-size: 11px; font-weight: 500; color: var(--elyra-muted); text-align: center; }
  td {
    height: 32px; padding: 0; text-align: center; border-radius: calc(var(--elyra-radius) - 2px);
    color: var(--elyra-text); cursor: pointer; font-variant-numeric: tabular-nums;
  }
  td:hover { background: var(--elyra-accent-soft); }
  td:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: -2px; }
  td.outside { color: var(--elyra-muted); }
  td[aria-current="date"] { font-weight: 700; color: var(--elyra-accent); }
  td[aria-selected="true"] { background: var(--elyra-accent); color: var(--elyra-on-accent); }
  td[aria-disabled="true"] { opacity: .35; cursor: default; background: none; }
  .foot { display: flex; justify-content: flex-end; margin-top: 4px; }
  .text {
    padding: 4px 8px; border: 0; border-radius: calc(var(--elyra-radius) - 2px);
    background: transparent; color: var(--elyra-accent); font: inherit; cursor: pointer;
  }
  .text:hover:not(:disabled) { background: var(--elyra-accent-soft); }
  .text:disabled { color: var(--elyra-muted); cursor: default; }
</style>
