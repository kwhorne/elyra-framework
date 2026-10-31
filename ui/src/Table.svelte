<!--
  A table over a `live.*` page, the shape `make:resource` lists have:

    <Table page={live.customers_index(query)} bind:query columns={["name", "email"]} sortable search />

  Search, sort and page are written into `query` (`search`, `sort`,
  `direction`, `page`), so the live query runs again; the command sorts by
  the columns it allows. Loading, an error and an empty list each show as
  such. `rows` (a plain array) works too, searched, sorted and paged here.

  `columns` are keys, or `{ key, label, sortable, align, format(value, row) }`;
  `cell(row, column)` and `actions(row)` are snippets; `onrowclick(row)` makes
  the rows a list ↑/↓ and Enter work in. `labels` translates its words:
  `search`, `failed`, `empty`, `noMatch(term)`, `total(n)`, `previous`,
  `next`, `page(n)`, `selected(n)`, `clear`, `selectAll`, `selectRow(row)`.

  `select` gives the rows checkboxes, and `bind:selected` is their keys,
  kept across pages and searches. `select="customers"` also tells the
  assistant what's selected (RFC 0013). `bulk(selected, clear)` is a
  snippet for what to do with them.
-->
<script>
  import { untrack } from "svelte";
  import { assistant } from "@elyra/runtime";
  import Callout from "./Callout.svelte";
  import Checkbox from "./Checkbox.svelte";
  import Button from "./Button.svelte";
  import Empty from "./Empty.svelte";
  import Icon from "./internal/Icon.svelte";
  import Input from "./Input.svelte";
  import Pagination from "./Pagination.svelte";
  import Skeleton from "./Skeleton.svelte";

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
    labels = {},
    class: className = "",
  } = $props();

  const words = $derived({
    search: "Search…",
    failed: "Couldn't load this list",
    empty: "Nothing here yet",
    noMatch: (term) => `Nothing matches “${term}”`,
    total: (n) => `${n} in all`,
    previous: "Previous",
    next: "Next",
    page: (n) => `Page ${n}`,
    selected: (n) => `${n} selected`,
    clear: "Clear",
    selectAll: "Select all on this page",
    selectRow: (row) => `Select ${row[cols[0]?.key] ?? row[key]}`,
    ...labels,
  });

  /** `{ value, error, loading }`, from a live store, a `Live`, or a plain `Page`. */
  let live = $state({ value: undefined, error: null, loading: false });
  $effect(() => {
    if (page?.subscribe) return page.subscribe((state) => (live = state));
    live = page && "data" in page ? { value: page, error: null, loading: false } : page ?? { value: undefined, error: null, loading: false };
  });

  const cols = $derived(
    columns.map((c) => {
      const col = typeof c === "string" ? { key: c } : { ...c };
      col.label ??= col.key.replace(/_/g, " ").replace(/^./, (s) => s.toUpperCase());
      col.sortable ??= sortable === true || (Array.isArray(sortable) && sortable.includes(col.key));
      return col;
    }),
  );

  // A plain array: searched, sorted and paged here.
  const local = $derived.by(() => {
    if (!rows) return null;
    const term = (query.search ?? "").trim().toLowerCase();
    let list = term
      ? rows.filter((r) => cols.some((c) => String(r[c.key] ?? "").toLowerCase().includes(term)))
      : rows.slice();
    if (query.sort) {
      const dir = query.direction === "desc" ? -1 : 1;
      list.sort((a, b) => {
        const x = a[query.sort];
        const y = b[query.sort];
        return (x > y ? 1 : x < y ? -1 : 0) * dir;
      });
    }
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

  function onRowKey(event, row, index) {
    // Keys on what's inside the row (its checkbox, its buttons) are theirs.
    if (event.target !== event.currentTarget) return;
    if (selecting && event.key === " ") {
      event.preventDefault();
      toggle(index, event);
    } else if (selecting && event.key.toLowerCase() === "a" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      mark(pageKeys, true);
    } else if (selecting && event.key === "Escape" && selected.length) {
      event.preventDefault();
      clear();
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onrowclick?.(row);
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const next = event.key === "ArrowDown" ? event.currentTarget.nextElementSibling : event.currentTarget.previousElementSibling;
      next?.focus();
    }
  }

  const sortState = (col) =>
    query.sort === col.key ? (query.direction === "desc" ? "descending" : "ascending") : col.sortable ? "none" : undefined;
  const display = (row, col) => (col.format ? col.format(row[col.key], row) : row[col.key] ?? "");
</script>

<div class="elyra-table {className}">
  {#if search}
    <div class="search">
      <Input type="search" value={query.search ?? ""} placeholder={words.search} aria-label="Search" oninput={onSearch} />
    </div>
  {/if}

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

  <div class="scroll">
    <table aria-label={label} aria-busy={loading || (live.loading && !local) || undefined}>
      <thead>
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
            <th scope="col" aria-sort={sortState(col)} class:end={col.align === "end"}>
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
            </th>
          {/each}
          {#if actions}<th scope="col"><span class="hidden">Actions</span></th>{/if}
        </tr>
      </thead>
      <tbody>
        {#if loading}
          {#each { length: 3 } as _}
            <tr class="placeholder">
              {#if selecting}<td></td>{/if}
              {#each cols as col (col.key)}<td><Skeleton /></td>{/each}
              {#if actions}<td></td>{/if}
            </tr>
          {/each}
        {:else}
          {#each data as row, index (row[key] ?? row)}
            {@const on = selecting && chosen.has(keyOf(row))}
            <tr
              class:clickable={!!onrowclick || selecting}
              class:selected={on}
              tabindex={onrowclick || selecting ? 0 : undefined}
              onclick={onrowclick ? () => onrowclick(row) : selecting ? (event) => onRowClick(event, index) : undefined}
              onkeydown={onrowclick || selecting ? (event) => onRowKey(event, row, index) : undefined}
            >
              {#if selecting}
                <!-- The checkbox is the row's own: a click on it doesn't open the row. -->
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
                <td class="pick" onclick={(event) => event.stopPropagation()}>
                  <Checkbox
                    checked={on}
                    aria-label={words.selectRow(row)}
                    onclick={(event) => {
                      toggle(index, event);
                      event.currentTarget.checked = chosen.has(keyOf(row));
                    }}
                  />
                </td>
              {/if}
              {#each cols as col (col.key)}
                <td class:end={col.align === "end"}>
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
  .search { max-width: 280px; }
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
  .arrow { display: inline-flex; opacity: 0; transform: rotate(180deg); transition: opacity .12s, transform .12s; }
  .sort:hover .arrow { opacity: .5; }
  .arrow.on { opacity: 1; }
  .arrow.desc { transform: none; }
  tr.clickable { cursor: pointer; }
  tr.clickable:hover td { background: var(--elyra-inset); }
  tr.clickable:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: -2px; }
  tr.selected td { background: var(--elyra-accent-soft); }
  .pick { width: 1%; padding-right: 0; }
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
</style>
