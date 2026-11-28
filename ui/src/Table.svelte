<!--
  A table over a `live.*` page, the shape `make:resource` lists have:

    <Table page={live.customers_index(query)} bind:query columns={["name", "email"]} sortable search />

  Search, sort and page are written into `query` (`search`, `sort`,
  `direction`, `page`), so the live query runs again; the command sorts by
  the columns it allows. Loading, an error and an empty list each show as
  such. `rows` (a plain array) works too, searched, sorted and paged here.

  `columns` are keys, or `{ key, label, sortable, align, width, format(value, row) }`;
  `cell(row, column)` and `actions(row)` are snippets; `onrowclick(row)` makes
  the rows a list ↑/↓ and Enter work in. `labels` translates its words:
  `search`, `failed`, `empty`, `noMatch(term)`, `total(n)`, `previous`,
  `next`, `page(n)`, `selected(n)`, `clear`, `selectAll`, `selectRow(row)`,
  `resize(label)`, `pending`.

  `resizable` gives each column's header a grip to drag (or move with the
  arrow keys) to set its width; the last column, or the actions, takes the
  rest. `persist="key"` keeps the widths in the app's settings store.

  `pending` is the keys of rows with a change not yet sent (RFC 0039's
  `$pending.customers`): a dot at the start of each, "Not sent yet".

  `preview(row)` is a row's route for Quick Look (RFC 0041): Space shows
  it in a read-only window beside this one (⌘Y / Ctrl+Y with `select`),
  ↓/↑ move it along, Space or Esc closes it; Enter opens the row.

  `select` gives the rows checkboxes, and `bind:selected` is their keys,
  kept across pages and searches. `select="customers"` also tells the
  assistant what's selected (RFC 0013). `bulk(selected, clear)` is a
  snippet for what to do with them.

  Taking it out (RFC 0066): a context menu (Copy, Copy table, Save as
  CSV…), ⌘C / Ctrl+C on a focused row (it, or the selected rows), and
  `csv()`, `copy(rows?)`, `save()`. Numbers go plain, in the locale's
  decimal sign, so a spreadsheet sums them. `exportable={false}` turns it off.

  Totals (RFC 0067): a column's `total` (`"sum"`, `"avg"`, `"min"`, `"max"`,
  `"count"`, or `(rows, column) => value`) is in the table's foot, over every
  row as searched; a live page's `totals` from the server, else its own
  ("This page"); and a line for the selected rows. Printed once, and the
  last line of the CSV and Copy table.

  Columns the user picks (RFC 0068): `choosable` gives a Columns menu to
  show, hide and order them (Alt+↑/↓ on an item, or a header dragged), and
  `persist` keeps the choice with the widths. A column's `hidden` starts it
  hidden, `required` keeps it shown. The search goes on over every column;
  the export, the totals and print are the shown ones.

  Filters (RFC 0069): a column's `filter` (`"text"`, `{ options }` or
  `"options"`, `"range"`, `"dates"`) gives its header a funnel, a panel to
  set it, and a chip over the table while it's on. They go in
  `query.filters`: a `rows` table filters itself, a live page's command
  gets them.
-->
<script>
  import { tick, untrack } from "svelte";
  import { assistant, clipboard, contextMenu, files, paths, quickLook, store, toast } from "@elyra/runtime";
  import Callout from "./Callout.svelte";
  import Checkbox from "./Checkbox.svelte";
  import Button from "./Button.svelte";
  import Empty from "./Empty.svelte";
  import Icon from "./internal/Icon.svelte";
  import Input from "./Input.svelte";
  import Pagination from "./Pagination.svelte";
  import Skeleton from "./Skeleton.svelte";
  import { useAppLocale, useWords } from "./internal/words.js";
  import { csvText, fileNameOf, plainNumber, tsvText } from "./internal/csv.js";
  import { hasTotal, totalText, totalsOf } from "./internal/totals.js";
  import { arrange, canHide, keptOf, moved, toggled } from "./internal/columns.js";
  import { activeFilters, chipText, filterRows, isEmpty, kindOf, optionsOf } from "./internal/filters.js";
  import { focusables, onOutside } from "./internal/focus.js";
  import { place } from "./internal/place.js";
  import DateRangePicker from "./DateRangePicker.svelte";
  import Dropdown from "./Dropdown.svelte";
  import MenuItem from "./MenuItem.svelte";

  let {
    page = undefined,
    rows = undefined,
    query = $bindable({}),
    columns = [],
    sortable = false,
    search = false,
    perPage = 25,
    key = "id",
    label = undefined,
    onrowclick = undefined,
    cell = undefined,
    actions = undefined,
    empty = undefined,
    select = false,
    selected = $bindable([]),
    bulk = undefined,
    resizable = false,
    persist = undefined,
    pending = undefined,
    preview = undefined,
    exportable = true,
    choosable = false,
    labels = {},
    class: className = "",
  } = $props();

  const words = useWords("table", () => labels);

  /** A row with a change not yet sent (RFC 0039). */
  const unsent = (row) => !!pending?.some((k) => String(k) === String(row[key]));

  /** `{ value, error, loading }`, from a live store, a `Live`, or a plain `Page`. */
  let live = $state({ value: undefined, error: null, loading: false });
  $effect(() => {
    if (page?.subscribe) return page.subscribe((state) => (live = state));
    live = page && "data" in page ? { value: page, error: null, loading: false } : page ?? { value: undefined, error: null, loading: false };
  });

  /** The app's columns, in its order. */
  const appCols = $derived(
    columns.map((c) => {
      const col = typeof c === "string" ? { key: c } : { ...c };
      col.label ??= col.key.replace(/_/g, " ").replace(/^./, (s) => s.toUpperCase());
      col.sortable ??= sortable === true || (Array.isArray(sortable) && sortable.includes(col.key));
      return col;
    }),
  );

  // Columns the user picks (RFC 0068): the choice, and whether the table is
  // on paper (a report is as the app set it, whatever the user chose).
  let choice = $state.raw(null);
  let root = $state();
  const onPaper = $derived(!!root?.closest?.(".elyra-paper"));
  const arranged = $derived(arrange(appCols, choosable && !onPaper ? choice : null));
  /** The columns shown, in the user's order. */
  const cols = $derived(arranged.shown);

  // Widths: what the user set (or the store kept), else the column's own
  // `width`. The last column, or the actions, takes the rest, so it has none.
  const MIN_WIDTH = 48;
  const PICK_WIDTH = 36;
  /** The narrowest a column without a width is given room for. */
  const AUTO_WIDTH = 120;
  let widths = $state({});
  const flexKey = $derived(actions ? null : cols[cols.length - 1]?.key);
  const widthOf = (col) => (col.key === flexKey ? undefined : (widths[col.key] ?? col.width));
  const sized = $derived(cols.some((c) => widthOf(c) != null));
  const tableWidth = $derived.by(() => {
    if (!sized) return undefined;
    let sum = selecting ? PICK_WIDTH : 0;
    let auto = actions ? 1 : 0;
    for (const c of cols) {
      const w = widthOf(c);
      if (w == null) auto++;
      else sum += w;
    }
    return `max(100%, ${sum + auto * AUTO_WIDTH}px)`;
  });
  /** The header cells, to measure when a resize starts. */
  const heads = {};
  const widthKey = $derived((resizable || choosable) && persist ? `elyra-ui.table.${persist}` : null);

  // The widths and the columns the user left, from the settings store (not
  // there in a browser): `{ widths, hidden, order }`, or 0.62's widths alone.
  $effect(() => {
    if (!widthKey) return;
    let live = true;
    Promise.resolve()
      .then(() => store.get(widthKey))
      .then((saved) => {
        if (!live) return;
        const kept = keptOf(saved);
        if (resizable) {
          const own = {};
          for (const [k, w] of Object.entries(kept.widths)) own[k] = Math.max(MIN_WIDTH, Math.round(w));
          widths = own;
        }
        if (choosable && kept.choice) choice = kept.choice;
      })
      .catch(() => {});
    return () => (live = false);
  });

  let saving;
  function saveWidths(wait = 0) {
    if (!widthKey) return;
    clearTimeout(saving);
    const at = widthKey;
    const kept = $state.snapshot(widths);
    // With `choosable`, one value for the table's look; without, the widths as 0.62 kept them.
    const value = choosable ? { widths: kept, hidden: choice?.hidden ?? [], order: choice?.order ?? [] } : kept;
    saving = setTimeout(() => Promise.resolve().then(() => store.set(at, value)).catch(() => {}), wait);
  }
  $effect(() => () => clearTimeout(saving));

  /** Before the first change, every column but the one that takes the rest keeps the width it has. */
  function freeze() {
    for (const c of cols) {
      if (c.key === flexKey || widthOf(c) != null) continue;
      widths[c.key] = Math.max(MIN_WIDTH, Math.round(heads[c.key]?.getBoundingClientRect().width || 0));
    }
  }
  const setWidth = (col, w) => (widths[col.key] = Math.max(MIN_WIDTH, Math.round(w)));

  let resizing = $state(null);
  function onGripDown(event, col) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    freeze();
    resizing = { key: col.key, from: event.clientX, start: widthOf(col) };
  }
  function onGripMove(event, col) {
    if (resizing?.key !== col.key) return;
    setWidth(col, resizing.start + event.clientX - resizing.from);
  }
  function onGripUp(event) {
    if (!resizing) return;
    resizing = null;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    saveWidths();
  }
  function onGripKey(event, col) {
    const by = event.shiftKey ? 64 : 16;
    if (event.key === "Enter") return resetWidth(event, col);
    const keys = { ArrowLeft: -by, ArrowRight: by };
    if (!(event.key in keys) && event.key !== "Home") return;
    event.preventDefault();
    freeze();
    setWidth(col, event.key === "Home" ? MIN_WIDTH : widthOf(col) + keys[event.key]);
    saveWidths(300);
  }
  /** Back to the column's own width, or none. */
  function resetWidth(event, col) {
    event.preventDefault();
    delete widths[col.key];
    saveWidths();
  }

  // A plain array: searched, sorted and paged here.
  /** Every row the table has, searched and sorted: what it exports (RFC 0066). */
  const sorted = $derived.by(() => {
    if (!rows) return null;
    const term = (query.search ?? "").trim().toLowerCase();
    // The filters first (RFC 0069), then the search.
    const kept = filterRows(rows, query.filters, appCols);
    let list = term
      ? kept.filter((r) => appCols.some((c) => String(r[c.key] ?? "").toLowerCase().includes(term)))
      : kept.slice();
    if (query.sort) {
      const dir = query.direction === "desc" ? -1 : 1;
      list.sort((a, b) => {
        const x = a[query.sort];
        const y = b[query.sort];
        return (x > y ? 1 : x < y ? -1 : 0) * dir;
      });
    }
    return list;
  });
  const local = $derived.by(() => {
    if (!sorted) return null;
    const list = sorted;
    const last = Math.max(1, Math.ceil(list.length / perPage));
    const current = Math.min(Math.max(1, query.page ?? 1), last);
    return {
      data: list.slice((current - 1) * perPage, current * perPage),
      total: list.length,
      per_page: perPage,
      current_page: current,
      last_page: last,
    };
  });

  const result = $derived(local ?? live.value);
  const data = $derived(result?.data ?? []);
  const loading = $derived(!local && live.loading && !live.value);

  // Selection: the rows' keys, kept across pages and searches.
  const selecting = $derived(!!select);
  const kind = $derived(typeof select === "string" ? select : null);
  const keyOf = (row) => row[key];
  const chosen = $derived(new Set(selected));
  const pageKeys = $derived(data.map(keyOf));
  const allOnPage = $derived(pageKeys.length > 0 && pageKeys.every((k) => chosen.has(k)));
  const someOnPage = $derived(!allOnPage && pageKeys.some((k) => chosen.has(k)));
  /** The row on this page the last click or Space was on: Shift+click selects from there. */
  let anchor = null;

  function mark(keys, on) {
    const next = new Set(selected);
    for (const k of keys) on ? next.add(k) : next.delete(k);
    selected = [...next];
  }

  function toggle(index, event) {
    const on = !chosen.has(pageKeys[index]);
    if (event?.shiftKey && anchor !== null && anchor < pageKeys.length) {
      const [from, to] = anchor < index ? [anchor, index] : [index, anchor];
      mark(pageKeys.slice(from, to + 1), on);
    } else {
      mark([pageKeys[index]], on);
    }
    anchor = index;
  }

  /** A click on the row selects it, unless it was on something of its own in a cell. */
  function onRowClick(event, index) {
    if (event.target.closest?.("input, button, a, select, textarea, label, [contenteditable]")) return;
    toggle(index, event);
  }

  function clear() {
    selected = [];
    anchor = null;
  }

  // A row that's gone is no longer selected: from a plain array, any row not
  // in it; from a live page, a row the same page and query no longer has.
  let seen = { at: null, keys: [] };
  $effect(() => {
    const keys = pageKeys;
    const at = JSON.stringify([query, result?.current_page]);
    const busy = !local && live.loading;
    untrack(() => {
      if (!selecting || selected.length === 0) {
        seen = { at, keys };
        return;
      }
      let gone;
      if (local) {
        const all = new Set(rows.map(keyOf));
        gone = selected.filter((k) => !all.has(k));
      } else if (seen.at === at && !busy) {
        const now = new Set(keys);
        gone = seen.keys.filter((k) => !now.has(k));
      }
      if (gone?.length) {
        const drop = new Set(gone);
        selected = selected.filter((k) => !drop.has(k));
      }
      seen = { at, keys };
    });
  });

  // `select="customers"`: the assistant's note says what's selected. Once
  // something has been, an empty selection is said too, and leaving clears it.
  let told = false;
  // A runtime from before 0.22 has no `assistant.selected`, and an app
  // without an assistant refuses: either way the table goes on.
  const tell = (name, ids) => Promise.resolve().then(() => assistant.selected(name, ids)).catch(() => {});
  $effect(() => {
    const ids = [...selected];
    const name = kind;
    if (!name || (ids.length === 0 && !told)) return;
    const timer = setTimeout(() => {
      told = true;
      tell(name, ids);
    }, 150);
    return () => clearTimeout(timer);
  });
  $effect(() => {
    const name = kind;
    return () => {
      if (name && told) tell(name, []);
      told = false;
    };
  });

  function sortBy(col) {
    if (!col.sortable) return;
    const direction = query.sort === col.key && query.direction !== "desc" ? "desc" : "asc";
    query = { ...query, sort: col.key, direction, page: 1 };
  }

  let typing;
  function onSearch(event) {
    const value = event.currentTarget.value;
    clearTimeout(typing);
    typing = setTimeout(() => (query = { ...query, search: value, page: 1 }), rows ? 0 : 250);
  }

  // Quick Look (RFC 0041): this table opened it, and it follows the rows.
  let looking = $state(false);
  $effect(() => quickLook.subscribe((q) => { if (!q.open) looking = false; }));
  $effect(() => () => { if (looking) void quickLook.hide().catch(() => {}); });
  const look = (row) => {
    const route = preview?.(row);
    if (!route) return;
    looking = true;
    void quickLook.show(route).catch(() => { looking = false; });
  };

  // Esc closes it wherever the focus is in the window (the search field, a
  // row's checkbox, the toolbar), not only on a row: its window never takes
  // the focus. Not while a dialog, menu or list that Esc is for is open.
  function onWindowKey(event) {
    if (!looking || event.key !== "Escape" || event.defaultPrevented || event.isComposing) return;
    const target = event.target instanceof Element ? event.target : null;
    const owned = '[role="dialog"], dialog, [role="menu"], [role="listbox"], [aria-modal="true"]';
    if (target?.closest(owned) || document.querySelector('dialog[open], [aria-modal="true"]')) return;
    event.preventDefault();
    looking = false;
    void quickLook.hide().catch(() => {});
  }

  function onRowKey(event, row, index) {
    // Keys on what's inside the row (its checkbox, its buttons) are theirs.
    if (event.target !== event.currentTarget) return;
    if (exportKeys(event, row)) return;
    // Space previews, or ⌘Y / Ctrl+Y where Space selects.
    const previewKey = preview && (selecting
      ? event.key.toLowerCase() === "y" && (event.metaKey || event.ctrlKey)
      : event.key === " ");
    if (previewKey) {
      event.preventDefault();
      if (looking) {
        looking = false;
        void quickLook.hide().catch(() => {});
      } else {
        look(row);
      }
    } else if (looking && event.key === "Escape") {
      event.preventDefault();
      looking = false;
      void quickLook.hide().catch(() => {});
    } else if (selecting && event.key === " ") {
      event.preventDefault();
      toggle(index, event);
    } else if (selecting && event.key.toLowerCase() === "a" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      mark(pageKeys, true);
    } else if (selecting && event.key === "Escape" && selected.length) {
      event.preventDefault();
      clear();
    } else if (event.key === "Enter" || (event.key === " " && !preview)) {
      event.preventDefault();
      onrowclick?.(row);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const down = event.key === "ArrowDown";
      const next = down ? event.currentTarget.nextElementSibling : event.currentTarget.previousElementSibling;
      next?.focus();
      const nextRow = data[index + (down ? 1 : -1)];
      if (looking && next && nextRow) look(nextRow);
    }
  }

  // --- Taking it out (RFC 0066) ---

  const appLocale = useAppLocale();
  const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/;

  /**
   * A cell as a spreadsheet takes it: a number plain, in the locale's
   * decimal sign; a date as its ISO string; else the column's `format`, or
   * the value as text. Never a cell snippet's drawing.
   */
  function plainOf(row, col, number) {
    const value = row[col.key];
    if (typeof value === "number") return Number.isFinite(value) ? number(value) : "";
    if (typeof value === "string" && ISO_DATE.test(value)) return value;
    if (col.format) return String(col.format(value, row) ?? "");
    if (value === null || value === undefined) return "";
    if (typeof value === "object" && !(value instanceof Date)) return JSON.stringify(value);
    return String(value);
  }

  /** The rows as lines of cells, the columns' labels first with `head`, and the foot's first line with it. */
  function linesOf(list, head, cols = arranged.shown) {
    const number = plainNumber(appLocale());
    const lines = list.map((row) => cols.map((col) => plainOf(row, col, number)));
    if (!head) return lines;
    const total = cols.some(hasTotal) ? totalLine() : null;
    const foot = total
      ? cols.map((col, at) => {
          const value = total.values[col.key];
          if (typeof value === "number") return Number.isFinite(value) ? number(value) : "";
          if (value !== null && value !== undefined) return String(value);
          return at === 0 ? total.name : "";
        })
      : null;
    return [cols.map((col) => col.label), ...lines, ...(foot ? [foot] : [])];
  }

  // --- Totals (RFC 0067) ---

  // Every column's total, hidden ones too (for `csv({ all: true })`); a foot while a shown one has one.
  const totalled = $derived(appCols.filter(hasTotal));
  /**
   * The foot's lines: the total (over every row as searched; a live page's
   * `totals`, else its own rows, "This page"), and the selected rows'.
   */
  /** The foot's first line, every column's: null while there are no rows. */
  function totalLine() {
    if (totalled.length === 0 || loading || data.length === 0) return null;
    if (sorted) return { name: words.totalRow, values: totalsOf(sorted, totalled) };
    if (result?.totals && typeof result.totals === "object") return { name: words.totalRow, values: result.totals };
    return { name: words.pageTotal, values: totalsOf(data, totalled) };
  }
  const footLines = $derived.by(() => {
    const first = cols.some(hasTotal) ? totalLine() : null;
    if (!first) return [];
    const lines = [first];
    if (selecting && selected.length) {
      const picked = (sorted ?? data).filter((row) => chosen.has(keyOf(row)));
      if (picked.length) lines.push({ name: words.selectedTotal(picked.length), values: totalsOf(picked, totalled), selection: true });
    }
    return lines;
  });

  /** What the table has: every row of a plain array, searched and sorted; a live query's page. */
  const everyRow = () => sorted ?? data;

  /** The row's, or the selected rows when it's one of them (or there's no row). */
  function rowsFor(row) {
    if (selecting && selected.length && (!row || chosen.has(keyOf(row)))) return everyRow().filter((r) => chosen.has(keyOf(r)));
    return row ? [row] : [];
  }

  /** The table as CSV, its label the first line, in the app's locale: the shown columns, or with `all`, every one in the app's order. */
  export function csv({ all = false } = {}) {
    return csvText([...(label ? [[label]] : []), ...linesOf(everyRow(), true, all ? appCols : arranged.shown)], appLocale());
  }

  // --- Columns the user picks (RFC 0068) ---

  let announced = $state("");
  let chooser = $state();

  function choose(next) {
    choice = next;
    saveWidths();
  }

  /** Alt+↑/↓ on a column's item: it moves, and says where. */
  function onChoiceKey(event, col) {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    event.stopPropagation();
    shift(col, event.key === "ArrowUp" ? -1 : 1);
  }

  function shift(col, by) {
    const next = moved(arranged, col.key, { by });
    choose(next);
    announced = words.moved(col.label, next.order.indexOf(col.key) + 1, next.order.length);
  }

  /** A right click on the head opens the Columns menu. */
  function onHeadMenu(event) {
    if (!choosable || onPaper) return;
    if (event.target instanceof Element && event.target.closest("input, textarea")) return;
    event.preventDefault();
    event.stopPropagation();
    chooser?.querySelector("button")?.click();
  }

  // A header dragged to its place (the pointer; the keys have the menu).
  let dragging = $state(null);
  let swallowClick = false;
  function onHeadDown(event, col) {
    if (!choosable || onPaper || event.button !== 0) return;
    if (event.target instanceof Element && event.target.closest(".grip")) return;
    dragging = { key: col.key, from: event.clientX, moving: false };
  }
  function onHeadMove(event) {
    if (!dragging) return;
    if (!dragging.moving && Math.abs(event.clientX - dragging.from) < 6) return;
    dragging.moving = true;
    // The shown column under the pointer: the dragged one goes to its place.
    const over = cols.find((c) => {
      const rect = heads[c.key]?.getBoundingClientRect();
      return rect && event.clientX >= rect.left && event.clientX < rect.right;
    });
    if (!over || over.key === dragging.key) return;
    const to = arranged.all.findIndex((c) => c.key === over.key);
    choice = moved(arranged, dragging.key, { to });
  }
  function onHeadUp() {
    if (!dragging) return;
    if (dragging.moving) {
      swallowClick = true;
      setTimeout(() => (swallowClick = false), 0);
      const col = appCols.find((c) => c.key === dragging.key);
      announced = words.moved(col.label, (choice?.order ?? []).indexOf(col.key) + 1, appCols.length);
      saveWidths();
    }
    dragging = null;
  }
  /** The click that ends a drag doesn't sort. */
  function onHeadClick(event) {
    if (!swallowClick) return;
    event.preventDefault();
    event.stopPropagation();
  }

  /** Sorted by a column the user hid: its header isn't there to say so. */
  // --- Filters (RFC 0069) ---

  /** The filters that filter something, by column key. */
  const active = $derived(activeFilters(query.filters, appCols));
  const activeCols = $derived(appCols.filter((c) => c.key in active));
  /** The column whose filter panel is open, the funnel it's under, and what's being set. */
  let filtering = $state(null);
  let funnel = $state();
  let filterPanel = $state();
  let draft = $state(null);

  function emptyDraft(kind) {
    if (kind === "options") return [];
    if (kind === "range") return { min: null, max: null };
    if (kind === "dates") return { from: null, to: null };
    return "";
  }

  function openFilter(col, button) {
    if (filtering?.key === col.key) return closeFilter();
    const kind = kindOf(col);
    const now = active[col.key];
    draft = now === undefined ? emptyDraft(kind) : $state.snapshot(now);
    funnel = button;
    filtering = { key: col.key, kind, col };
  }

  function closeFilter(refocus = true) {
    if (!filtering) return;
    filtering = null;
    if (refocus) funnel?.focus();
  }

  $effect(() => {
    if (!filtering) return;
    tick().then(() => (focusables(filterPanel)[0] ?? filterPanel)?.focus());
    return onOutside(() => [filterPanel, funnel], () => closeFilter(false));
  });

  /** Write a column's filter (or none) into the query, from the first page. */
  function setFilter(key, value) {
    const next = activeFilters({ ...(query.filters ?? {}), [key]: value }, appCols);
    query = { ...query, filters: Object.keys(next).length ? next : null, page: 1 };
  }

  function applyFilter() {
    const { key, kind } = filtering;
    const value = kind === "range" ? numbers(draft) : $state.snapshot(draft);
    setFilter(key, isEmpty(kind, value) ? undefined : value);
    closeFilter();
  }

  function clearFilter(key) {
    setFilter(key, undefined);
    if (filtering?.key === key) closeFilter();
  }

  const numbers = (r) => ({ min: toNumber(r.min), max: toNumber(r.max) });
  const toNumber = (v) => (v === null || v === undefined || v === "" || !Number.isFinite(Number(v)) ? null : Number(v));

  function onFilterKey(event) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeFilter();
    } else if (event.key === "Enter" && event.target instanceof HTMLInputElement && event.target.type !== "checkbox") {
      event.preventDefault();
      applyFilter();
    }
  }

  function toggleOption(value) {
    const has = draft.some((v) => v === value);
    draft = has ? draft.filter((v) => v !== value) : [...draft, value];
  }

  // What's shown, said as the filters change: "3 of 412 shown".
  let lastShown = null;
  $effect(() => {
    const filtersNow = JSON.stringify(active);
    const shown = sorted ? sorted.length : (result?.total ?? null);
    const total = rows ? rows.length : null;
    untrack(() => {
      if (lastShown !== null && lastShown !== filtersNow && shown !== null) {
        announced = total !== null ? words.shown(shown, total) : words.total(shown);
      }
      lastShown = filtersNow;
    });
  });

  const sortedHidden = $derived(
    query.sort && !cols.some((c) => c.key === query.sort) ? appCols.find((c) => c.key === query.sort) : null,
  );

  /** `rows` on the clipboard, tab-separated; without them, the table with its head. */
  export async function copy(list) {
    const text = tsvText(list ? linesOf(list, false) : linesOf(everyRow(), true));
    if (await hasSystem()) await clipboard.writeText(text);
    else await navigator.clipboard.writeText(text);
  }

  /** Save the table as CSV, where the user chooses. The path written, or null if they cancelled. */
  export async function save() {
    return files.save(csv(), {
      defaultName: fileNameOf({ label, extension: "csv", fallback: "table" }),
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
  }

  /** Whether the shell can save files and hold text: asked once, by the page. */
  let system = null;
  const hasSystem = () =>
    (system ??= paths().then(
      () => true,
      () => false,
    ));

  async function attempt(run, done) {
    try {
      const result = await run();
      if (done && result !== null) toast(done, { variant: "success" });
    } catch (error) {
      toast(words.exportFailed(error?.message ?? String(error)), { variant: "danger" });
    }
  }

  const textSelected = () => (typeof window !== "undefined" && window.getSelection?.()?.toString()) || "";
  const ownMenu = "input, textarea, select, [contenteditable]";

  /** The menu: Copy (the row, or the selected rows), Copy table, Save as CSV… in the shell. */
  async function openMenu(event, row) {
    if (!exportable || loading || everyRow().length === 0) return;
    if (event.target instanceof Element && event.target.closest(ownMenu)) return;
    // The browser's own menu isn't shown, whatever the shell answers.
    event.preventDefault();
    const target = rowsFor(row);
    const full = await hasSystem();
    const items = [];
    if (target.length) items.push({ label: words.copy(target.length), action: () => attempt(() => copy(target), words.copied) });
    items.push({ label: words.copyTable, action: () => attempt(() => copy(), words.copied) });
    if (full) items.push({ separator: true }, { label: words.saveCsv, action: () => attempt(save) });
    contextMenu(event, items);
  }

  function onTableMenu(event) {
    const at = event.target instanceof Element ? event.target.closest("tr[data-row]") : null;
    void openMenu(event, at ? data[Number(at.dataset.row)] : undefined);
  }

  /** ⌘C, Shift+F10 and the Menu key on a row: true if it was one of them. */
  function exportKeys(event, row) {
    if (!exportable) return false;
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "c" && !event.altKey && !textSelected()) {
      event.preventDefault();
      void attempt(() => copy(rowsFor(row)), words.copied);
      return true;
    }
    if ((event.key === "F10" && event.shiftKey) || event.key === "ContextMenu") {
      const rect = event.currentTarget.getBoundingClientRect();
      void openMenu(new MouseEvent("contextmenu", { clientX: rect.left + 16, clientY: rect.top + rect.height / 2, cancelable: true }), row);
      event.preventDefault();
      return true;
    }
    return false;
  }

  const sortState = (col) =>
    query.sort === col.key ? (query.direction === "desc" ? "descending" : "ascending") : col.sortable ? "none" : undefined;
  const display = (row, col) => (col.format ? col.format(row[col.key], row) : row[col.key] ?? "");
</script>

<svelte:window onkeydown={onWindowKey} onpointermove={onHeadMove} onpointerup={onHeadUp} onpointercancel={onHeadUp} />

<div class="elyra-table {className}" class:resizing={!!resizing} class:moving={dragging?.moving} bind:this={root}>
  {#if search || (choosable && !onPaper)}
    <div class="tools">
      {#if search}
        <div class="search">
          <Input type="search" value={query.search ?? ""} placeholder={words.search} aria-label={words.searchLabel} oninput={onSearch} />
        </div>
      {/if}
      {#if choosable && !onPaper}
        <div class="chooser" bind:this={chooser}>
          <Dropdown label={words.columns} size="sm" variant="ghost" align="end">
            {#each arranged.all as col, i (col.key)}
              {@const shown = cols.some((c) => c.key === col.key)}
              <div class="choice">
                <button
                  type="button"
                  role="menuitemcheckbox"
                  tabindex="-1"
                  aria-checked={shown}
                  disabled={shown && !canHide(arranged, col.key)}
                  aria-keyshortcuts="Alt+ArrowUp Alt+ArrowDown"
                  onclick={() => choose(toggled(arranged, col.key))}
                  onkeydown={(event) => onChoiceKey(event, col)}
                >
                  <span class="check" aria-hidden="true">{#if shown}<Icon name="check" size={12} />{/if}</span>
                  {col.label}
                </button>
                <!-- For the pointer: the keys move a column with Alt+↑/↓ on its item. -->
                <span class="order" aria-hidden="true">
                  <button type="button" tabindex="-1" title={words.moveUp} disabled={i === 0} onclick={() => shift(col, -1)}>↑</button>
                  <button type="button" tabindex="-1" title={words.moveDown} disabled={i === arranged.all.length - 1} onclick={() => shift(col, 1)}>↓</button>
                </span>
              </div>
            {/each}
            <MenuItem onclick={() => { choice = null; widths = {}; saveWidths(); }}>{words.resetColumns}</MenuItem>
          </Dropdown>
        </div>
      {/if}
    </div>
  {/if}
  {#if activeCols.length}
    <!-- The filters that are on, each with its way off (RFC 0069). -->
    <div class="chips">
      <ul aria-label={words.filters}>
        {#each activeCols as col (col.key)}
          {@const text = chipText(col, active[col.key], { words, locale: appLocale(), rows: rows ?? data })}
          <li class="chip">
            <span>{text}</span>
            <button type="button" aria-label={words.removeFilter(text)} title={words.removeFilter(text)} onclick={() => clearFilter(col.key)}>
              <Icon name="close" size={10} />
            </button>
          </li>
        {/each}
      </ul>
      {#if activeCols.length > 1}
        <Button size="sm" variant="ghost" onclick={() => (query = { ...query, filters: null, page: 1 })}>{words.clearFilters}</Button>
      {/if}
    </div>
  {/if}

  {#if filtering}
    <div
      bind:this={filterPanel}
      class="filter-panel"
      role="dialog"
      aria-label={words.filter(filtering.col.label)}
      tabindex="-1"
      use:place={{ anchor: funnel, align: "start", gap: 6 }}
      onkeydown={onFilterKey}
    >
      {#if filtering.kind === "text"}
        <Input type="search" bind:value={draft} aria-label={filtering.col.label} />
      {:else if filtering.kind === "options"}
        <div class="options" role="group" aria-label={filtering.col.label}>
          {#each optionsOf(filtering.col, rows ?? data) as [value, text], i (i)}
            <Checkbox checked={draft.some((v) => v === value)} onchange={() => toggleOption(value)}>{text}</Checkbox>
          {/each}
        </div>
      {:else if filtering.kind === "range"}
        <div class="range">
          <Input type="number" bind:value={draft.min} aria-label={words.from} placeholder={words.from} />
          <span aria-hidden="true">–</span>
          <Input type="number" bind:value={draft.max} aria-label={words.to} placeholder={words.to} />
        </div>
      {:else if filtering.kind === "dates"}
        <DateRangePicker bind:start={draft.from} bind:end={draft.to} presets={[]} />
      {/if}
      <div class="filter-actions">
        <Button size="sm" variant="ghost" onclick={() => clearFilter(filtering.key)}>{words.clearFilter}</Button>
        <Button size="sm" variant="primary" onclick={applyFilter}>{words.apply}</Button>
      </div>
    </div>
  {/if}

  <div class="said hidden" role="status" aria-live="polite">{announced}</div>

  {#if selecting && selected.length}
    <div class="bulk" role="region" aria-label={words.selected(selected.length)}>
      <span class="count" aria-live="polite">{words.selected(selected.length)}</span>
      <div class="bulk-actions">
        {@render bulk?.(selected, clear)}
        <Button size="sm" variant="ghost" onclick={clear}>{words.clear}</Button>
      </div>
    </div>
  {/if}

  {#if !local && live.error}
    <Callout variant="danger" title={words.failed}>{live.error.message ?? String(live.error)}</Callout>
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="scroll" oncontextmenu={onTableMenu}>
    <table
      aria-label={label}
      aria-description={sortedHidden ? words.sortedBy(sortedHidden.label) : undefined}
      aria-busy={loading || (live.loading && !local) || undefined}
      class:fixed={sized}
      style:--elyra-table-width={tableWidth}
    >
      {#if sized}
        <colgroup>
          {#if selecting}<col style:width="{PICK_WIDTH}px" />{/if}
          {#each cols as col (col.key)}<col style:width={widthOf(col) == null ? undefined : `${widthOf(col)}px`} />{/each}
          {#if actions}<col />{/if}
        </colgroup>
      {/if}
      <thead oncontextmenu={onHeadMenu}>
        <tr>
          {#if selecting}
            <th scope="col" class="pick">
              <Checkbox
                checked={allOnPage}
                indeterminate={someOnPage}
                disabled={pageKeys.length === 0}
                aria-label={words.selectAll}
                onclick={(event) => {
                  mark(pageKeys, !allOnPage);
                  event.currentTarget.checked = allOnPage;
                }}
              />
            </th>
          {/if}
          {#each cols as col (col.key)}
            <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
            <th
              scope="col"
              aria-sort={sortState(col)}
              class:end={col.align === "end"}
              class:dragged={dragging?.moving && dragging.key === col.key}
              bind:this={heads[col.key]}
              onpointerdown={(event) => onHeadDown(event, col)}
              onclickcapture={onHeadClick}
            >
              {#if col.sortable}
                <button type="button" class="sort" onclick={() => sortBy(col)}>
                  {col.label}
                  <span class="arrow" class:on={query.sort === col.key} class:desc={query.sort === col.key && query.direction === "desc"}>
                    <Icon name="chevron-down" size={12} />
                  </span>
                </button>
              {:else}
                {col.label}
              {/if}
              {#if kindOf(col) && !onPaper}
                <button
                  type="button"
                  class="funnel"
                  class:on={col.key in active}
                  aria-label={col.key in active ? words.filtered(col.label) : words.filter(col.label)}
                  aria-haspopup="dialog"
                  aria-expanded={filtering?.key === col.key}
                  onclick={(event) => openFilter(col, event.currentTarget)}
                >
                  <Icon name="filter" size={12} />
                </button>
              {/if}
              {#if resizable && col.key !== flexKey}
                <!-- A focusable separator is a control (a column splitter): it takes the keys. -->
                <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
                <span
                  class="grip"
                  class:active={resizing?.key === col.key}
                  role="separator"
                  tabindex="0"
                  aria-orientation="vertical"
                  aria-label={words.resize(col.label)}
                  aria-valuenow={widthOf(col)}
                  aria-valuemin={MIN_WIDTH}
                  onpointerdown={(event) => onGripDown(event, col)}
                  onpointermove={(event) => onGripMove(event, col)}
                  onpointerup={onGripUp}
                  onpointercancel={onGripUp}
                  onkeydown={(event) => onGripKey(event, col)}
                  ondblclick={(event) => resetWidth(event, col)}
                ></span>
              {/if}
            </th>
          {/each}
          {#if actions}<th scope="col" class="actions"><span class="hidden">{words.actions}</span></th>{/if}
        </tr>
      </thead>
      <tbody>
        {#if loading}
          {#each { length: 3 } as _}
            <tr class="placeholder">
              {#if selecting}<td class="pick"></td>{/if}
              {#each cols as col (col.key)}<td><Skeleton /></td>{/each}
              {#if actions}<td class="actions"></td>{/if}
            </tr>
          {/each}
        {:else}
          {#each data as row, index (row[key] ?? row)}
            {@const on = selecting && chosen.has(keyOf(row))}
            <tr
              data-row={index}
              class:clickable={!!onrowclick || selecting}
              class:selected={on}
              tabindex={onrowclick || selecting || preview ? 0 : undefined}
              onclick={onrowclick ? () => onrowclick(row) : selecting ? (event) => onRowClick(event, index) : undefined}
              onkeydown={onrowclick || selecting || preview ? (event) => onRowKey(event, row, index) : undefined}
            >
              {#if selecting}
                <!-- The checkbox is the row's own: a click on it doesn't open the row. -->
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
                <td class="pick" onclick={(event) => event.stopPropagation()}>
                  <Checkbox
                    checked={on}
                    aria-label={words.selectRow(row, row[cols[0]?.key] ?? row[key])}
                    onclick={(event) => {
                      toggle(index, event);
                      event.currentTarget.checked = chosen.has(keyOf(row));
                    }}
                  />
                </td>
              {/if}
              {#each cols as col, at (col.key)}
                <td class:end={col.align === "end"}>
                  {#if at === 0 && unsent(row)}<span class="pending" role="img" aria-label={words.pending} title={words.pending}></span>{/if}
                  {#if cell}{@render cell(row, col)}{:else}{display(row, col)}{/if}
                </td>
              {/each}
              {#if actions}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
                <td class="actions" onclick={(event) => event.stopPropagation()}>{@render actions(row)}</td>
              {/if}
            </tr>
          {/each}
        {/if}
      </tbody>
      {#if footLines.length}
        <tfoot>
          {#each footLines as line (line.name)}
            <tr class:selection={line.selection}>
              {#if selecting}<td class="pick"></td>{/if}
              {#each cols as col, at (col.key)}
                {@const text = totalText(line.values[col.key], col, appLocale())}
                {#if at === 0}
                  <th scope="row" class:end={col.align === "end" && hasTotal(col)}>{hasTotal(col) ? `${line.name}: ${text}` : line.name}</th>
                {:else}
                  <td class:end={col.align === "end"}>{text}</td>
                {/if}
              {/each}
              {#if actions}<td class="actions"></td>{/if}
            </tr>
          {/each}
        </tfoot>
      {/if}
    </table>
  </div>

  {#if !loading && data.length === 0 && !(live.error && !local)}
    {#if empty}{@render empty()}{:else}<Empty title={query.search ? words.noMatch(query.search) : words.empty} />{/if}
  {/if}

  {#if result && result.last_page > 1}
    <div class="foot">
      <span class="total">{words.total(result.total)}</span>
      <Pagination
        page={result.current_page}
        lastPage={result.last_page}
        previousLabel={words.previous}
        nextLabel={words.next}
        pageLabel={words.page}
        onchange={(p) => (query = { ...query, page: p })}
      />
    </div>
  {/if}
</div>

<style>
  .elyra-table { display: grid; gap: 10px; font: 13px/1.45 var(--elyra-font); color: var(--elyra-text); min-width: 0; }
  .search { max-width: 280px; flex: 1; }
  .tools { display: flex; align-items: center; gap: 8px; }
  .chooser { margin-left: auto; }
  .choice { display: flex; align-items: center; }
  .choice [role="menuitemcheckbox"] {
    flex: 1; display: flex; align-items: center; gap: 8px; min-height: 28px; padding: 0 10px; border: 0;
    border-radius: calc(var(--elyra-radius) - 2px); background: none; color: var(--elyra-text);
    font: 13px var(--elyra-font); text-align: left; cursor: pointer;
  }
  .choice [role="menuitemcheckbox"]:focus-visible, .choice [role="menuitemcheckbox"]:hover:not(:disabled) { background: var(--elyra-raised); outline: none; }
  .choice [role="menuitemcheckbox"]:disabled { color: var(--elyra-muted); cursor: default; }
  .check { width: 12px; display: inline-flex; color: var(--elyra-accent); }
  .order { display: inline-flex; }
  .order button {
    width: 22px; height: 22px; border: 0; background: none; color: var(--elyra-muted); cursor: pointer; border-radius: 4px;
  }
  .order button:hover:not(:disabled) { color: var(--elyra-text); background: var(--elyra-raised); }
  .order button:disabled { opacity: .35; cursor: default; }
  .moving { user-select: none; }
  .moving th { cursor: grabbing; }
  th.dragged { color: var(--elyra-accent); }
  /* Filters (RFC 0069). */
  .funnel {
    display: inline-flex; align-items: center; justify-content: center; width: 20px; height: 20px; margin-left: 2px;
    vertical-align: middle; border: 0; border-radius: 4px; background: none; color: var(--elyra-muted); opacity: .55; cursor: pointer;
  }
  th:hover .funnel, .funnel:focus-visible, .funnel[aria-expanded="true"] { opacity: 1; }
  .funnel.on { opacity: 1; color: var(--elyra-accent); }
  .funnel:hover { background: var(--elyra-raised); }
  .funnel:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  .chips { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .chips ul { display: flex; gap: 6px; flex-wrap: wrap; margin: 0; padding: 0; list-style: none; }
  .chip {
    display: inline-flex; align-items: center; gap: 4px; height: 24px; padding: 0 4px 0 10px; border-radius: 12px;
    background: var(--elyra-accent-soft); color: var(--elyra-text); font-size: 12px;
  }
  .chip button {
    display: inline-flex; align-items: center; justify-content: center; width: 18px; height: 18px; border: 0;
    border-radius: 9px; background: none; color: var(--elyra-muted); cursor: pointer;
  }
  .chip button:hover { color: var(--elyra-text); background: var(--elyra-raised); }
  .chip button:focus-visible { outline: 2px solid var(--elyra-focus); }
  .filter-panel {
    position: absolute; z-index: 60; display: grid; gap: 10px; min-width: 220px; padding: 12px;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); background: var(--elyra-raised);
    color: var(--elyra-text); box-shadow: var(--elyra-shadow); outline: none;
  }
  .filter-panel .options { display: grid; gap: 6px; max-height: 240px; overflow: auto; }
  .filter-panel .range { display: flex; align-items: center; gap: 6px; }
  .filter-panel .range :global(.elyra-input) { width: 100px; }
  .filter-actions { display: flex; justify-content: flex-end; gap: 6px; }
  .scroll { overflow-x: auto; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius); }
  table { width: 100%; border-collapse: collapse; font-variant-numeric: tabular-nums; }
  th {
    position: sticky; top: 0; z-index: 1; padding: 0 12px; height: 34px; text-align: left; white-space: nowrap;
    background: var(--elyra-surface); color: var(--elyra-muted); font-size: 12px; font-weight: 600;
    border-bottom: 1px solid var(--elyra-border);
  }
  td { padding: 8px 12px; border-bottom: 1px solid var(--elyra-border); vertical-align: middle; }
  tbody tr:last-child td { border-bottom: 0; }
  .end { text-align: right; }
  .sort {
    display: inline-flex; align-items: center; gap: 4px; padding: 0; border: 0; background: none;
    color: inherit; font: inherit; cursor: pointer;
  }
  .sort:hover { color: var(--elyra-text); }
  .sort:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; border-radius: 3px; }
  .arrow { display: inline-flex; opacity: 0; transform: rotate(180deg); transition: opacity var(--elyra-duration-fast), transform var(--elyra-duration-fast); }
  .sort:hover .arrow { opacity: .5; }
  .arrow.on { opacity: 1; }
  .arrow.desc { transform: none; }
  tr.clickable { cursor: pointer; }
  tr.clickable:hover td { background: var(--elyra-inset); }
  tr.clickable:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: -2px; }
  tr.selected td { background: var(--elyra-accent-soft); }
  .pick { width: 1%; padding-right: 0; }
  .pending {
    display: inline-block;
    width: 6px;
    height: 6px;
    margin-inline-end: 6px;
    border-radius: 50%;
    background: var(--elyra-accent);
    vertical-align: middle;
  }
  table.fixed { table-layout: fixed; width: var(--elyra-table-width); }
  table.fixed th, table.fixed td:not(.actions):not(.pick) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grip {
    position: absolute; top: 0; right: 0; width: 7px; height: 100%; cursor: col-resize;
    touch-action: none; outline: none;
  }
  .grip::after { content: ""; position: absolute; top: 9px; bottom: 9px; right: 0; width: 1px; background: var(--elyra-border); }
  .grip:hover::after, .grip.active::after, .grip:focus-visible::after { width: 2px; background: var(--elyra-accent); }
  .resizing { user-select: none; cursor: col-resize; }
  .bulk {
    display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap;
    padding: 6px 6px 6px 12px; border-radius: var(--elyra-radius);
    background: var(--elyra-accent-soft); border: 1px solid color-mix(in srgb, var(--elyra-accent) 35%, transparent);
  }
  .count { font-weight: 500; font-variant-numeric: tabular-nums; }
  .bulk-actions { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .actions { text-align: right; white-space: nowrap; }
  .actions :global(.elyra-button + .elyra-button) { margin-left: 6px; }
  .hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  .foot { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .total { color: var(--elyra-muted); font-variant-numeric: tabular-nums; }
  /*
   * On paper (RFC 0062): every loaded row, the head again on each page, no
   * row split; what's for the hand (search, selection, actions, paging,
   * resizing) stays behind.
   */
  /* Totals (RFC 0067): under the columns, a line above them. */
  tfoot th, tfoot td {
    padding: 0 12px; height: 36px; text-align: left; white-space: nowrap; font-weight: 600;
    background: var(--elyra-surface); color: var(--elyra-text); border-top: 1px solid var(--elyra-border);
  }
  tfoot th { position: static; font-size: 13px; }
  tfoot .end { text-align: right; }
  tfoot tr.selection th, tfoot tr.selection td { color: var(--elyra-muted); border-top-style: dashed; }
  @media print {
    .tools, .bulk, .foot, .grip, .pick, .actions, .funnel, .chips button, tfoot tr.selection { display: none; }
    /* Once, after the last row: not at the foot of every page. */
    tfoot { display: table-row-group; }
    tfoot th, tfoot td { background: none; }
    .scroll { overflow: visible; border-radius: 0; }
    table.fixed { table-layout: auto; width: 100%; }
    table.fixed th, table.fixed td:not(.actions):not(.pick) { white-space: normal; overflow: visible; }
    thead { display: table-header-group; }
    th { position: static; background: none; }
    tr { break-inside: avoid; }
    tr.selected td, tr.clickable:hover td { background: none; }
  }
  /* In a report (RFC 0064), on screen too: the print form. */
  :global(.elyra-paper) .tools, :global(.elyra-paper) .bulk, :global(.elyra-paper) .foot,
  :global(.elyra-paper) .grip, :global(.elyra-paper) .pick, :global(.elyra-paper) .actions,
  :global(.elyra-paper) tfoot tr.selection, :global(.elyra-paper) .chips button { display: none; }
  :global(.elyra-paper) tfoot th, :global(.elyra-paper) tfoot td { background: none; }
  :global(.elyra-paper) .scroll { overflow: visible; border-radius: 0; }
  :global(.elyra-paper) th { position: static; background: none; }
</style>
