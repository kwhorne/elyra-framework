<!--
  A tree: folders, categories, an organisation. `role="tree"`, with the
  keyboard WAI-ARIA describes: ↓/↑ through the rows that show, → opens a
  branch or moves to its first child, ← closes it or moves to its parent,
  Home/End, Enter or Space selects, * opens a row's siblings, and typing
  moves to the next row whose label starts with what was typed.

  Each node is `{ id, label, children?, can?, disabled? }`. A node with
  `children` is a branch; one with `hasChildren: true` is a branch whose
  children `load(node)` reads the first time it opens (an array, or a
  `Page<T>`). `persist="key"` keeps which branches are open in the app's
  settings store, so the tree opens as the user left it.
-->
<script>
  import { tick, untrack } from "svelte";
  import { can as canStore, store } from "@elyra/runtime";
  import Icon from "./internal/Icon.svelte";

  let {
    nodes = [],
    selected = $bindable(null),
    expanded = $bindable([]),
    load = undefined,
    persist = undefined,
    label = undefined,
    onselect = undefined,
    labels = {},
    class: className = "",
    item = undefined,
    ...rest
  } = $props();

  const own = $props.id();
  const words = $derived({ loading: "Loading…", failed: "Couldn't load this.", retry: "Try again", ...labels });
  const keyOf = (node) => String(node.id);
  const open = $derived(new Set(expanded.map(String)));

  // What `load` gave each lazy branch: loading, its nodes, or what went wrong.
  let loads = $state({});

  const childrenOf = (node) =>
    Array.isArray(node.children) ? node.children : node.hasChildren ? (loads[keyOf(node)]?.nodes ?? null) : null;
  const isBranch = (node) => Array.isArray(node.children) || (node.hasChildren === true && !!load);
  const allowed = (node) => !node.can || $canStore(node.can);

  /** The rows that show, in order: each with its level, its parent, and its place among its siblings. */
  const rows = $derived.by(() => {
    const out = [];
    const walk = (list, level, parent) => {
      const shown = list.filter(allowed);
      shown.forEach((node, i) => {
        const key = keyOf(node);
        const branch = isBranch(node);
        const row = { node, key, level, parent, branch, open: branch && open.has(key), size: shown.length, pos: i + 1 };
        out.push(row);
        const kids = row.open ? childrenOf(node) : null;
        if (kids) walk(kids, level + 1, key);
      });
    };
    walk(nodes ?? [], 1, null);
    return out;
  });
  const byKey = $derived(new Map(rows.map((r) => [r.key, r])));
  /** The rows under each parent (`null`: the top), for the nested groups. */
  const under = $derived.by(() => {
    const map = new Map();
    for (const row of rows) {
      if (!map.has(row.parent)) map.set(row.parent, []);
      map.get(row.parent).push(row);
    }
    return map;
  });

  // The row the keyboard is on: the one Tab reaches (a roving tabindex).
  let active = $state(null);
  const current = $derived(
    byKey.has(active) ? active : selected != null && byKey.has(String(selected)) ? String(selected) : (rows[0]?.key ?? null),
  );

  const elements = new Map();
  function register(element, key) {
    elements.set(key, element);
    return {
      update(next) {
        elements.delete(key);
        key = next;
        elements.set(key, element);
      },
      destroy: () => elements.get(key) === element && elements.delete(key),
    };
  }

  // Each row's label has an id: a treeitem is named by its label, not by its children's.
  const ids = new Map();
  const idOf = (key) => {
    if (!ids.has(key)) ids.set(key, `${own}-${ids.size}`);
    return ids.get(key);
  };

  async function focus(key) {
    if (key == null) return;
    active = key;
    await tick();
    elements.get(key)?.focus();
  }

  // Which branches are open, from the settings store (not there in a browser).
  const storeKey = $derived(persist ? `elyra-ui.tree.${persist}` : null);
  $effect(() => {
    if (!storeKey) return;
    let live = true;
    Promise.resolve()
      .then(() => store.get(storeKey))
      .then((saved) => {
        if (live && Array.isArray(saved)) expanded = saved;
      })
      .catch(() => {});
    return () => (live = false);
  });
  function save() {
    if (!storeKey) return;
    const value = $state.snapshot(expanded);
    Promise.resolve()
      .then(() => store.set(storeKey, value))
      .catch(() => {});
  }

  async function readChildren(node) {
    const key = keyOf(node);
    // Read once: loading, read, or failed (Try again clears it first).
    if (loads[key]) return;
    loads[key] = { state: "loading" };
    try {
      const got = await load(node);
      loads[key] = { state: "done", nodes: Array.isArray(got) ? got : Array.isArray(got?.data) ? got.data : [] };
    } catch (error) {
      loads[key] = { state: "error", error: error?.message ?? String(error) };
    }
  }

  // A lazy branch that shows open (opened, or remembered open) reads its children.
  $effect(() => {
    for (const row of rows) {
      const lazy = row.open && row.node.hasChildren && !Array.isArray(row.node.children);
      if (lazy) untrack(() => readChildren(row.node));
    }
  });

  function setOpen(row, on) {
    if (!row.branch || row.open === on) return;
    expanded = on ? [...expanded, row.node.id] : expanded.filter((id) => String(id) !== row.key);
    save();
  }

  function retry(row) {
    delete loads[row.key];
    readChildren(row.node);
  }

  function choose(row) {
    if (row.node.disabled) return;
    selected = row.node.id;
    onselect?.(row.node);
  }

  let typed = "";
  let typedAt = 0;
  function typeahead(char) {
    const now = Date.now();
    typed = now - typedAt > 700 ? char : typed + char;
    typedAt = now;
    const at = rows.findIndex((r) => r.key === current);
    // The same letter again ("sss") moves on to the next; a word ("sup") may stay put.
    const repeated = [...typed].every((c) => c === typed[0]);
    const prefix = (repeated ? typed[0] : typed).toLocaleLowerCase();
    const starts = (r) => String(r.node.label ?? "").toLocaleLowerCase().startsWith(prefix);
    const from = repeated ? at + 1 : at;
    const order = [...rows.slice(from), ...rows.slice(0, from)];
    const hit = order.find(starts);
    if (hit) focus(hit.key);
  }

  function onkeydown(event) {
    // Each row hears its own keys; a nested row's don't reach its parent's.
    if (event.target !== event.currentTarget || event.altKey || event.ctrlKey || event.metaKey) return;
    const at = rows.findIndex((r) => r.key === current);
    const row = rows[at];
    if (!row) return;
    let handled = true;
    switch (event.key) {
      case "ArrowDown":
        focus(rows[Math.min(at + 1, rows.length - 1)].key);
        break;
      case "ArrowUp":
        focus(rows[Math.max(at - 1, 0)].key);
        break;
      case "ArrowRight":
        if (!row.branch) break;
        if (!row.open) setOpen(row, true);
        else if (loads[row.key]?.state === "error") retry(row);
        else if (rows[at + 1]?.parent === row.key) focus(rows[at + 1].key);
        break;
      case "ArrowLeft":
        if (row.open) setOpen(row, false);
        else if (row.parent != null) focus(row.parent);
        break;
      case "Home":
        focus(rows[0].key);
        break;
      case "End":
        focus(rows[rows.length - 1].key);
        break;
      case "Enter":
      case " ":
        choose(row);
        break;
      case "*": {
        const siblings = rows.filter((r) => r.parent === row.parent && r.branch && !r.open);
        if (siblings.length) {
          expanded = [...expanded, ...siblings.map((r) => r.node.id)];
          save();
        }
        break;
      }
      default:
        if (event.key.length === 1 && /\S/.test(event.key)) typeahead(event.key);
        else handled = false;
    }
    if (handled) event.preventDefault();
  }

  function onclick(event, row) {
    // A click on a nested row is that row's, not its parent's.
    if (event.target.closest('[role="treeitem"]') !== event.currentTarget) return;
    active = row.key;
    if (event.target.closest(".toggle")) setOpen(row, !row.open);
    else choose(row);
  }

  function ondblclick(event, row) {
    if (event.target.closest('[role="treeitem"]') !== event.currentTarget || event.target.closest(".toggle")) return;
    setOpen(row, !row.open);
  }
</script>

{#snippet level(list)}
  {#each list as row (row.key)}
    {@const loaded = loads[row.key]}
    <li
      use:register={row.key}
      role="treeitem"
      tabindex={row.key === current ? 0 : -1}
      aria-labelledby={idOf(row.key)}
      aria-level={row.level}
      aria-setsize={row.size}
      aria-posinset={row.pos}
      aria-expanded={row.branch ? row.open : undefined}
      aria-selected={selected != null && String(selected) === row.key}
      aria-disabled={row.node.disabled ? true : undefined}
      aria-busy={loaded?.state === "loading" ? true : undefined}
      {onkeydown}
      onclick={(event) => onclick(event, row)}
      ondblclick={(event) => ondblclick(event, row)}
      onfocus={(event) => event.target === event.currentTarget && (active = row.key)}
    >
      <div class="row" style:--depth={row.level - 1}>
        {#if row.branch}
          <span class="toggle" class:open={row.open} aria-hidden="true"><Icon name="chevron-right" size={12} /></span>
        {:else}
          <span class="toggle leaf" aria-hidden="true"></span>
        {/if}
        <span class="label" id={idOf(row.key)}>
          {#if item}{@render item(row.node)}{:else}{row.node.label}{/if}
        </span>
      </div>
      {#if row.open}
        {#if loaded?.state === "loading"}
          <div class="note" style:--depth={row.level}>{words.loading}</div>
        {:else if loaded?.state === "error"}
          <div class="note failed" style:--depth={row.level}>
            <span>{words.failed}</span>
            <button type="button" tabindex="-1" onclick={(event) => (event.stopPropagation(), retry(row))}>{words.retry}</button>
          </div>
        {:else}
          {@const kids = under.get(row.key) ?? []}
          {#if kids.length}
            <ul role="group">{@render level(kids)}</ul>
          {/if}
        {/if}
      {/if}
    </li>
  {/each}
{/snippet}

<ul role="tree" aria-label={label} class="elyra-tree {className}" {...rest}>
  {@render level(under.get(null) ?? [])}
</ul>

<style>
  .elyra-tree { margin: 0; padding: 2px 0; list-style: none; font: 13px var(--elyra-font); color: var(--elyra-text); }
  ul { margin: 0; padding: 0; list-style: none; }
  li { outline: none; }
  .row {
    display: flex; align-items: center; gap: 4px; min-height: 28px; box-sizing: border-box;
    padding: 0 8px 0 calc(var(--depth) * 16px + 4px); border-radius: calc(var(--elyra-radius) - 2px);
    cursor: default; user-select: none;
  }
  li:hover > .row { background: color-mix(in srgb, var(--elyra-accent-soft) 60%, transparent); }
  li[aria-selected="true"] > .row { background: var(--elyra-accent-soft); }
  li:focus-visible > .row { outline: 2px solid var(--elyra-focus); outline-offset: -2px; }
  li[aria-disabled="true"] > .row { opacity: .5; }
  .toggle {
    display: inline-grid; place-items: center; flex: none; width: 16px; height: 16px;
    color: var(--elyra-muted); transition: transform var(--elyra-duration-fast) var(--elyra-ease);
  }
  .toggle.open { transform: rotate(90deg); }
  .label { display: inline-flex; align-items: center; gap: 6px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note {
    display: flex; align-items: center; gap: 8px; min-height: 26px;
    padding-left: calc(var(--depth) * 16px + 24px); color: var(--elyra-muted); font-size: 12px;
  }
  .note.failed span { color: var(--elyra-danger); }
  .note button {
    padding: 0; border: 0; background: none; color: var(--elyra-accent); font: inherit; cursor: pointer;
  }
  .note button:hover { text-decoration: underline; }
</style>
