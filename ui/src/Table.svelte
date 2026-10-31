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
  `next`, `page(n)`.
-->
<script>
  import Callout from "./Callout.svelte";
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

  function onRowKey(event, row) {
    if (event.key === "Enter" || event.key === " ") {
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

  {#if !local && live.error}
    <Callout variant="danger" title={words.failed}>{live.error.message ?? String(live.error)}</Callout>
  {/if}

  <div class="scroll">
    <table aria-label={label} aria-busy={loading || (live.loading && !local) || undefined}>
      <thead>
        <tr>
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
              {#each cols as col (col.key)}<td><Skeleton /></td>{/each}
              {#if actions}<td></td>{/if}
            </tr>
          {/each}
        {:else}
          {#each data as row (row[key] ?? row)}
            <tr
              class:clickable={!!onrowclick}
              tabindex={onrowclick ? 0 : undefined}
              onclick={onrowclick ? () => onrowclick(row) : undefined}
              onkeydown={onrowclick ? (event) => onRowKey(event, row) : undefined}
            >
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
  .actions { text-align: right; white-space: nowrap; }
  .actions :global(.elyra-button + .elyra-button) { margin-left: 6px; }
  .hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip: rect(0 0 0 0); }
  .foot { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  .total { color: var(--elyra-muted); font-variant-numeric: tabular-nums; }
</style>
