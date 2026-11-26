<!--
  A span of days: from `start` to `end`, each an ISO date (`2026-11-30`) or
  null, so a range can be open at one end (a filter: "since 1 March"). Both
  are typed as the locale writes dates, or picked in a calendar of two
  months: the first day picked, then the last, the span shown between them
  as the pointer or the focus moves. `presets` are spans a click away
  (the last 7 days, this month); a function of today gives them for when
  the calendar opens. The calendar's keys are DatePicker's: arrows a day or
  a week, Home/End the week's ends, PageUp/PageDown a month (Shift: a year),
  Enter picks, Esc closes. A start typed after the end swaps them.
-->
<script>
  import { getContext, tick, untrack } from "svelte";
  import Icon from "./internal/Icon.svelte";
  import { FIELD, FORM } from "./internal/context.js";
  import { onOutside, trapTab } from "./internal/focus.js";
  import { place } from "./internal/place.js";
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
    startOfMonth,
    startOfWeek,
    today as todayIso,
  } from "./internal/dates.js";
  import { defaultPresets } from "./internal/ranges.js";
  import { useAppLocale, useWords } from "./internal/words.js";

  let {
    start = $bindable(null),
    end = $bindable(null),
    min = undefined,
    max = undefined,
    locale: localeProp = undefined,
    weekStart = undefined,
    presets = defaultPresets,
    placeholder = undefined,
    clearable = true,
    disabled = false,
    onchange = undefined,
    labels = {},
    class: className = "",
  } = $props();

  const field = getContext(FIELD);
  const words = useWords("daterange", () => labels);
  const appLocale = useAppLocale();
  // The app's locale when none is given (RFC 0056), else the webview's.
  const locale = $derived(localeProp ?? appLocale());
  // Told when the user makes a choice: a `<Form>`'s `dirty`.
  const form = getContext(FORM);
  const own = $props.id();
  const titleId = `${own}-title`;
  const hintId = `${own}-hint`;

  const first = $derived(weekStart ?? localeWeekStart(locale));
  const hint = $derived(placeholder ?? numericPattern(locale));
  const from = $derived(parts(start) ? start : null);
  const to = $derived(parts(end) ? end : null);
  const today = todayIso();

  const long = (day) =>
    new Intl.DateTimeFormat(locale, { dateStyle: "full", timeZone: "UTC" }).format(new Date(`${day}T00:00:00Z`));
  const monthTitle = (day) =>
    new Intl.DateTimeFormat(locale, { month: "long", year: "numeric", timeZone: "UTC" }).format(
      new Date(`${day}T00:00:00Z`),
    );
  const weekdays = $derived.by(() => {
    const monday = startOfWeek("2026-11-04", first);
    const short = new Intl.DateTimeFormat(locale, { weekday: "short", timeZone: "UTC" });
    const full = new Intl.DateTimeFormat(locale, { weekday: "long", timeZone: "UTC" });
    return Array.from({ length: 7 }, (_, i) => {
      const date = new Date(`${addDays(monday, i)}T00:00:00Z`);
      return { short: short.format(date), long: full.format(date) };
    });
  });

  let fromText = $state("");
  let toText = $state("");
  let fromTyped = $state(false);
  let toTyped = $state(false);
  let open = $state(false);
  /** The day the calendar's focus is on, and the first of the left month. */
  let focus = $state(today);
  let view = $state(startOfMonth(today));
  /** The first day picked, while the last is being chosen; and where the pointer is. */
  let anchor = $state(null);
  let hover = $state(null);
  let root = $state();
  let dialog = $state();
  let opener = $state();

  const months = $derived([view, addMonths(view, 1)].map((m) => ({ first: m, grid: monthGrid(m, first) })));
  const outOfRange = (day) => (min && day < min) || (max && day > max);
  const shownPresets = $derived(presets === defaultPresets ? defaultPresets(today, words) : typeof presets === "function" ? presets(today) : (presets ?? []));

  /** The span drawn: the one being picked, or the value. */
  const span = $derived.by(() => {
    if (anchor) {
      const other = hover ?? anchor;
      return other < anchor ? [other, anchor] : [anchor, other];
    }
    return [from, to];
  });
  const inSpan = (day) => {
    const [a, b] = span;
    if (a && b) return day >= a && day <= b;
    return day === a || day === b;
  };

  // While nothing is being typed, the fields show the value.
  $effect(() => {
    const now = formatNumeric(from, locale);
    if (!untrack(() => fromTyped)) fromText = now;
  });
  $effect(() => {
    const now = formatNumeric(to, locale);
    if (!untrack(() => toTyped)) toText = now;
  });

  $effect(() => {
    if (open) return onOutside(() => [root], () => close(false));
  });

  /** Set both ends, in order; tell the form and the app when they changed. */
  function set(a, b) {
    if (a && b && b < a) [a, b] = [b, a];
    fromTyped = false;
    toTyped = false;
    fromText = formatNumeric(a, locale);
    toText = formatNumeric(b, locale);
    if (a === start && b === end) return;
    start = a;
    end = b;
    form?.changed();
    onchange?.({ start: a, end: b });
  }

  function shift(day) {
    // The focus stays in the two months shown: past the right one, they move on.
    if (day < view) view = startOfMonth(day);
    else if (day >= addMonths(view, 2)) view = startOfMonth(addMonths(day, -1));
  }

  async function show() {
    if (disabled || open) return;
    focus = clamp(from ?? to ?? today, min, max);
    view = startOfMonth(focus);
    anchor = null;
    hover = null;
    open = true;
    await tick();
    focusDay();
  }

  function close(back = true) {
    open = false;
    anchor = null;
    hover = null;
    if (back) opener?.focus();
  }

  async function focusDay() {
    await tick();
    dialog?.querySelector(`[data-date="${focus}"]`)?.focus();
  }

  function pick(day) {
    if (outOfRange(day)) return;
    if (!anchor) {
      anchor = day;
      hover = day;
      focus = day;
      return;
    }
    set(anchor, day);
    close();
  }

  function choose(preset) {
    const a = preset.start ? clamp(preset.start, min, max) : null;
    const b = preset.end ? clamp(preset.end, min, max) : null;
    set(a, b);
    close();
  }

  function move(to) {
    focus = to;
    if (anchor) hover = to;
    shift(to);
    focusDay();
  }

  function ongridkey(event) {
    const keys = {
      ArrowLeft: () => addDays(focus, -1),
      ArrowRight: () => addDays(focus, 1),
      ArrowUp: () => addDays(focus, -7),
      ArrowDown: () => addDays(focus, 7),
      Home: () => startOfWeek(focus, first),
      End: () => addDays(startOfWeek(focus, first), 6),
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

  function page(n) {
    view = addMonths(view, n);
    focus = clamp(addMonths(focus, n), min, max);
  }

  /** A typed end, taken: a date in range, empty for none, or back to the value. */
  function commit(which) {
    const typed = which === "start" ? fromTyped : toTyped;
    if (!typed) return;
    const t = (which === "start" ? fromText : toText).trim();
    let day = null;
    if (t) {
      day = parseTyped(t, locale);
      if (!day || outOfRange(day)) {
        if (which === "start") {
          fromTyped = false;
          fromText = formatNumeric(from, locale);
        } else {
          toTyped = false;
          toText = formatNumeric(to, locale);
        }
        return;
      }
    }
    if (which === "start") set(day, to);
    else set(from, day);
  }

  function clear() {
    set(null, null);
  }

  function oninputkey(which, event) {
    const typed = which === "start" ? fromTyped : toTyped;
    if (event.key === "Enter" && typed) {
      commit(which);
    } else if (event.key === "ArrowDown" && (event.altKey || !typed)) {
      event.preventDefault();
      commit(which);
      show();
    }
  }

  const said = $derived(
    from && to
      ? words.between(long(from), long(to))
      : from
        ? words.from(long(from))
        : to
          ? words.until(long(to))
          : null,
  );
</script>

<div class="elyra-daterangepicker {className}" class:disabled bind:this={root}>
  <div class="box" role="group" aria-labelledby={field?.labelId} aria-label={field ? undefined : words.dates}>
    <input
      bind:value={fromText}
      type="text"
      inputmode="numeric"
      autocomplete="off"
      spellcheck="false"
      id={field?.id}
      aria-label={words.start}
      required={field?.required || undefined}
      aria-invalid={field?.invalid || undefined}
      aria-describedby={field?.describedBy}
      placeholder={hint}
      {disabled}
      oninput={() => (fromTyped = true)}
      onkeydown={(e) => oninputkey("start", e)}
      onblur={() => commit("start")}
    />
    <span class="dash" aria-hidden="true">–</span>
    <input
      bind:value={toText}
      type="text"
      inputmode="numeric"
      autocomplete="off"
      spellcheck="false"
      aria-label={words.end}
      aria-invalid={field?.invalid || undefined}
      placeholder={hint}
      {disabled}
      oninput={() => (toTyped = true)}
      onkeydown={(e) => oninputkey("end", e)}
      onblur={() => commit("end")}
    />
    {#if clearable && (from || to) && !disabled}
      <button
        type="button"
        class="icon clear"
        tabindex="-1"
        aria-label={words.clear}
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
      aria-label={said ? words.chooseWith(said) : words.choose}
      aria-haspopup="dialog"
      aria-expanded={open}
      {disabled}
      onclick={() => (open ? close() : show())}
    >
      <Icon name="calendar" />
    </button>
  </div>

  {#if open}
    <div
      class="popup"
      class:presets={shownPresets.length}
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={hintId}
      bind:this={dialog}
      tabindex="-1"
      onkeydown={ondialogkey}
      use:place={{ anchor: root }}
    >
      {#if shownPresets.length}
        <ul class="list" aria-label={words.presets}>
          {#each shownPresets as preset (preset.label)}
            <li>
              <button
                type="button"
                class="preset"
                aria-pressed={(preset.start ?? null) === from && (preset.end ?? null) === to}
                onclick={() => choose(preset)}
              >
                {preset.label}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="calendar">
        <span id={titleId} class="title" aria-live="polite">
          {monthTitle(months[0].first)} – {monthTitle(months[1].first)}
        </span>
        <div class="months">
          {#each months as m, i (m.first)}
            {@const month = m.first.slice(0, 7)}
            <div class="month">
              <div class="month-head">
                {#if i === 0}
                  <button type="button" class="icon" aria-label={words.previous} onclick={() => page(-1)}>
                    <Icon name="chevron-left" />
                  </button>
                {:else}<span class="gap"></span>{/if}
                <span class="month-title" aria-hidden="true">{monthTitle(m.first)}</span>
                {#if i === months.length - 1}
                  <button type="button" class="icon" aria-label={words.next} onclick={() => page(1)}>
                    <Icon name="chevron-right" />
                  </button>
                {:else}<span class="gap"></span>{/if}
              </div>
              <table role="grid" aria-label={monthTitle(m.first)}>
                <thead>
                  <tr>
                    {#each weekdays as w (w.long)}
                      <th scope="col" abbr={w.long}>{w.short}</th>
                    {/each}
                  </tr>
                </thead>
                <tbody>
                  {#each m.grid as week (week[0])}
                    <tr>
                      {#each week as day (day)}
                        {#if day.slice(0, 7) !== month}
                          <td role="gridcell" class="blank" aria-hidden="true"></td>
                        {:else}
                          <!-- The grid's keys are on each day: one is in the tab order at a time. -->
                          <td
                            role="gridcell"
                            data-date={day}
                            tabindex={day === focus ? 0 : -1}
                            aria-label={long(day)}
                            aria-selected={inSpan(day)}
                            aria-disabled={outOfRange(day) || undefined}
                            aria-current={day === today ? "date" : undefined}
                            class:edge={day === span[0] || day === span[1]}
                            class:first={day === span[0]}
                            class:last={day === span[1] || (!span[1] && day === span[0])}
                            onclick={() => pick(day)}
                            onmouseenter={() => anchor && (hover = day)}
                            onfocus={() => anchor && (hover = day)}
                            onkeydown={ongridkey}
                          >
                            {Number(day.slice(8))}
                          </td>
                        {/if}
                      {/each}
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/each}
        </div>
        <div class="foot">
          <span id={hintId} class="step" aria-live="polite">{anchor ? words.pickLast : words.pickFirst}</span>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .elyra-daterangepicker { position: relative; font: 13px var(--elyra-font); }
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
    /* Room for a whole date in numbers, each. */
    flex: 0 1 11ch; min-width: 9ch; width: 0; height: 100%; padding: 0; border: 0; outline: none;
    background: transparent; color: var(--elyra-text); font: inherit; font-variant-numeric: tabular-nums;
  }
  input::placeholder { color: var(--elyra-muted); }
  .dash { color: var(--elyra-muted); padding: 0 2px; }
  .icon {
    display: grid; place-items: center; width: 24px; height: 24px; padding: 0;
    border: 0; border-radius: calc(var(--elyra-radius) - 2px);
    background: transparent; color: var(--elyra-muted); cursor: pointer;
  }
  .icon:hover:not(:disabled) { background: var(--elyra-accent-soft); color: var(--elyra-text); }
  .icon:focus-visible, .preset:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  .icon:disabled { cursor: default; }
  .clear { width: 20px; height: 20px; }
  /* What's left of the field goes before its buttons. */
  .box > button:first-of-type { margin-left: auto; }
  .popup {
    position: absolute; z-index: 50; left: 0; top: calc(100% + 4px); display: flex; gap: 8px; padding: 8px;
    box-sizing: border-box; outline: none; max-width: calc(100vw - 16px);
    background: var(--elyra-raised); border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    box-shadow: var(--elyra-shadow);
    animation: open var(--elyra-duration-fast) var(--elyra-ease);
  }
  /* Opens with a short drop into place; instant for less motion. */
  @keyframes open { from { opacity: 0; transform: translateY(-4px); } }
  .popup:global([data-side="top"]) { animation-name: open-up; }
  @keyframes open-up { from { opacity: 0; transform: translateY(4px); } }
  .list {
    display: flex; flex-direction: column; gap: 2px; margin: 0; padding: 0 8px 0 0; list-style: none;
    border-right: 1px solid var(--elyra-border); min-width: 120px;
  }
  .preset {
    width: 100%; padding: 6px 8px; border: 0; border-radius: calc(var(--elyra-radius) - 2px);
    background: transparent; color: var(--elyra-text); font: inherit; text-align: left; cursor: pointer; white-space: nowrap;
  }
  .preset:hover { background: var(--elyra-accent-soft); }
  .preset[aria-pressed="true"] { color: var(--elyra-accent); font-weight: 600; }
  .calendar { min-width: 0; }
  .title { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
  .months { display: flex; flex-wrap: wrap; gap: 16px; }
  .month { width: 232px; }
  .month-head { display: flex; align-items: center; justify-content: space-between; margin-bottom: 4px; }
  .month-title { font-weight: 600; color: var(--elyra-text); text-transform: capitalize; }
  .gap { width: 24px; }
  table { width: 100%; border-collapse: collapse; table-layout: fixed; }
  th { padding: 4px 0; font-size: 11px; font-weight: 500; color: var(--elyra-muted); text-align: center; }
  td {
    height: 30px; padding: 0; text-align: center;
    color: var(--elyra-text); cursor: pointer; font-variant-numeric: tabular-nums;
  }
  td.blank { cursor: default; }
  td:not(.blank):hover { background: var(--elyra-accent-soft); border-radius: calc(var(--elyra-radius) - 2px); }
  td:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: -2px; border-radius: calc(var(--elyra-radius) - 2px); }
  td[aria-current="date"] { font-weight: 700; color: var(--elyra-accent); }
  td[aria-selected="true"] { background: var(--elyra-accent-soft); }
  td.edge, td.edge:not(.blank):hover { background: var(--elyra-accent); color: var(--elyra-on-accent); }
  td.first { border-top-left-radius: calc(var(--elyra-radius) - 2px); border-bottom-left-radius: calc(var(--elyra-radius) - 2px); }
  td.last { border-top-right-radius: calc(var(--elyra-radius) - 2px); border-bottom-right-radius: calc(var(--elyra-radius) - 2px); }
  td[aria-disabled="true"] { opacity: .35; cursor: default; background: none; }
  .foot { display: flex; justify-content: flex-start; margin-top: 6px; }
  .step { color: var(--elyra-muted); font-size: 12px; }
</style>
