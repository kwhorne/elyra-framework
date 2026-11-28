<!--
  A report (RFC 0064): its content on pages of a paper's size, each with the
  app's `header` and `footer`, given `{ page, pages }`. The body's direct
  children are the units: each goes whole on a page where it fits, else on
  the next; a table or a list taller than the room left is split between
  its rows or items, the table's head again on the next page. On screen the
  pages are sheets, fitted to the width; on paper, and in `pdf()`, each is
  a page.

  The body renders once, out of sight, to be measured; the pages show
  copies of it, laid out again whenever it changes.
-->
<script>
  import { onMount, tick, untrack } from "svelte";
  import { paginate, paperOf, px, unitsOf } from "./internal/pages.js";
  import { useWords } from "./internal/words.js";

  let {
    paper = "a4",
    landscape = false,
    margins = {},
    scale = "fit",
    pages = $bindable(1),
    header = undefined,
    footer = undefined,
    labels = {},
    class: className = "",
    children,
  } = $props();

  const words = useWords("report", () => labels);

  const size = $derived(paperOf(paper, landscape));
  const edge = $derived({ top: 15, right: 15, bottom: 15, left: 15, ...margins });
  const sheet = $derived({ width: px(size.width), height: px(size.height) });
  const bodyWidth = $derived(sheet.width - px(edge.left) - px(edge.right));

  /** The pages: each a list of `{ block, from?, to?, clipped? }`. */
  let plan = $state.raw([[]]);
  let source = $state();
  let frame = $state();
  /** The report as `print` and `pdf` take it (RFC 0062): its paper is on it. */
  let root = $state();
  export const element = () => root;
  /** On screen, the sheets fitted to the width. */
  let fit = $state(1);
  const heads = [];
  const feet = [];

  const blocksOf = () => (source ? [...source.children] : []);
  const heightOf = (el) => el.getBoundingClientRect().height;

  /** The body's room on each page: the sheet less its margins, its header and its footer. */
  function rooms() {
    const inner = sheet.height - px(edge.top) - px(edge.bottom);
    const measured = plan.map((_, i) => inner - (heads[i] ? heightOf(heads[i]) : 0) - (feet[i] ? heightOf(feet[i]) : 0));
    return (page) => measured[Math.min(page, measured.length - 1)] ?? inner;
  }

  /** Lay the body out again: until the pages' headers and footers stop changing the room. */
  let laying = false;
  /** The body changed while it was being laid out: once more after. */
  let again = false;
  async function layout() {
    if (!source) return;
    if (laying) {
      again = true;
      return;
    }
    laying = true;
    try {
      for (let pass = 0; pass < 4; pass++) {
        const blocks = blocksOf().map((el) => {
          const height = heightOf(el);
          const units = unitsOf(el);
          if (!units) return { height };
          const parts = units.map(heightOf);
          return { height, units: parts, fixed: height - parts.reduce((a, b) => a + b, 0) };
        });
        const next = paginate(blocks, rooms());
        const same = JSON.stringify(next) === JSON.stringify(plan);
        plan = next;
        pages = next.length;
        await tick();
        if (same) break;
      }
      fill();
    } finally {
      laying = false;
      if (again) {
        again = false;
        void layout();
      }
    }
  }

  /**
   * A copy of a block for a page: without ids, and with only its rows `from`
   * up to `to`. A table's foot (its totals, RFC 0067) goes with its last part.
   */
  function copyOf(block, part) {
    const copy = block.cloneNode(true);
    for (const el of [copy, ...copy.querySelectorAll("[id]")]) el.removeAttribute?.("id");
    if (part.from !== undefined) {
      const units = unitsOf(copy) ?? [];
      units.forEach((unit, i) => {
        if (i < part.from || i >= part.to) unit.remove();
      });
      if (part.to < units.length) for (const foot of copy.querySelectorAll("tfoot")) foot.remove();
    }
    if (part.clipped) copy.setAttribute("data-clipped", "");
    return copy;
  }

  /** Put the copies on the pages. */
  function fill() {
    const blocks = blocksOf();
    const bodies = frame ? [...frame.querySelectorAll(":scope > .sheet > .body")] : [];
    bodies.forEach((body, i) => {
      body.replaceChildren(...(plan[i] ?? []).map((part) => copyOf(blocks[part.block], part)));
      for (const part of plan[i] ?? []) {
        if (part.clipped) console.warn("@elyra/ui Report: a block taller than a page is clipped", blocks[part.block]);
      }
    });
  }

  // Again when the body changes, the paper does, or the fonts arrive: once
  // for a burst of changes (a chart drawing, a live query's rows).
  let pending;
  const schedule = () => {
    clearTimeout(pending);
    pending = setTimeout(() => void layout(), 30);
  };
  $effect(() => {
    void size;
    void edge;
    if (!source) return;
    untrack(() => void layout());
    const observer = typeof MutationObserver === "function" ? new MutationObserver(schedule) : null;
    observer?.observe(source, { childList: true, subtree: true, characterData: true, attributes: true });
    return () => {
      observer?.disconnect();
      clearTimeout(pending);
    };
  });

  onMount(() => {
    document.fonts?.ready?.then(schedule);
    if (scale !== "fit" || typeof ResizeObserver !== "function" || !root) return;
    const resize = new ResizeObserver(() => (fit = Math.min(1, root.clientWidth / sheet.width || 1)));
    resize.observe(root);
    return () => resize.disconnect();
  });

  const zoom = $derived(scale === "fit" ? fit : Number(scale) || 1);
</script>

<div
  class="elyra-report {className}"
  bind:this={root}
  data-elyra-report="{paper} {landscape ? 'landscape' : 'portrait'}"
  style:--elyra-report-zoom={zoom}
>
  <!-- Measured, not seen: the pages below are copies of it. -->
  <div class="source elyra-paper" bind:this={source} aria-hidden="true" style:width="{bodyWidth}px">{@render children?.()}</div>
  <div class="frame" bind:this={frame}>
    {#each plan as _, i (i)}
      <section
        class="sheet elyra-paper"
        role="group"
        aria-label={words.page(i + 1, plan.length)}
        style:width="{sheet.width}px"
        style:height="{sheet.height}px"
        style:padding="{px(edge.top)}px {px(edge.right)}px {px(edge.bottom)}px {px(edge.left)}px"
      >
        <div class="head" bind:this={heads[i]}>{@render header?.({ page: i + 1, pages: plan.length })}</div>
        <div class="body"></div>
        <div class="foot" bind:this={feet[i]}>{@render footer?.({ page: i + 1, pages: plan.length })}</div>
      </section>
    {/each}
  </div>
</div>

<style>
  .elyra-report { display: grid; justify-items: center; min-width: 0; overflow: hidden; }
  .source { position: absolute; left: -100000px; top: 0; visibility: hidden; }
  .frame { display: grid; gap: 16px; zoom: var(--elyra-report-zoom, 1); }
  .sheet {
    display: flex; flex-direction: column; box-sizing: border-box; overflow: hidden;
    background: #fff; color: var(--elyra-text); box-shadow: var(--elyra-shadow-md, 0 2px 10px rgba(0, 0, 0, .25));
    font: var(--elyra-font-size) / 1.5 var(--elyra-font);
  }
  .body { flex: 1; min-height: 0; overflow: hidden; }
  .head, .foot { flex: none; }
  .body :global([data-clipped]) { max-height: 100%; overflow: hidden; }

  /* On paper (RFC 0062): a sheet is a page. */
  @media print {
    .source { display: none; }
    .elyra-report { display: block; overflow: visible; }
    .frame { display: block; zoom: 1; }
    .sheet { box-shadow: none; break-after: page; }
    .sheet:last-child { break-after: auto; }
  }
</style>
