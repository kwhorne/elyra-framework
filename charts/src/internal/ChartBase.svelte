<!--
  What every chart is (RFC 0058): ECharts in an element of its own, the
  option rebuilt from the data and the theme; a live query's states; a
  summary as its name and its data as a table the screen reader and the
  assistant read; ←/→ along the categories, Enter to select.
-->
<script>
  import { onMount, untrack } from "svelte";
  import * as echarts from "echarts/core";
  import { SVGRenderer, CanvasRenderer } from "echarts/renderers";
  import { AriaComponent, GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
  import { Callout, Empty, Skeleton } from "@elyra/ui";
  import { clipboard, contextMenu, files, paths, toast } from "@elyra/runtime";
  import { csvOf, fileNameOf, pngFromSvg, pngOfCanvas, svgOf } from "./export.js";
  import { useAppLocale, useWords } from "@elyra/ui/internal/words.js";
  import { chartData, hasPoints, summaryOf, tableOf, valueOf } from "./data.js";
  import { themeOf, watchTheme } from "./theme.js";
  import { WORDS } from "./words.js";

  let {
    kind,
    use = [],
    build,
    series = [],
    categories = [],
    live = undefined,
    height = 320,
    label = undefined,
    format = undefined,
    locale: localeProp = undefined,
    renderer = "svg",
    onselect = undefined,
    labels = {},
    class: className = "",
    props = {},
    describe = undefined,
    onchart = undefined,
    unavailable = null,
    exportable = true,
    children = undefined,
  } = $props();

  // What every chart draws with; each chart adds its own part (`use`).
  echarts.use([SVGRenderer, CanvasRenderer, AriaComponent, GridComponent, LegendComponent, TooltipComponent]);

  const words = useWords("charts", () => labels, WORDS);
  const appLocale = useAppLocale();
  const locale = $derived(localeProp ?? appLocale());

  /** `{ value, error, loading }`, from a live store, a `Live`, or a plain value. */
  let state = $state({ value: undefined, error: null, loading: false });
  /** What ECharts threw drawing this chart: said here, and the page's other charts go on. */
  let drawError = $state(null);
  function draw(chart, opts) {
    try {
      chart.setOption(option(), opts);
      drawError = null;
    } catch (error) {
      drawError = error;
      console.error("@elyra/charts:", error);
    }
  }
  $effect(() => {
    if (live?.subscribe) return live.subscribe((s) => (state = s ?? { value: undefined, error: null, loading: false }));
    state = live === undefined ? { value: undefined, error: null, loading: false } : { value: live, error: null, loading: false };
  });

  // A WireCharts chart (RFC 0059) takes its live value as it is, and says
  // what it shows from its option (`describe`); the six of 0.58 from their series.
  const data = $derived(describe ? { value: state.value } : chartData({ series, categories }, state.value));
  const described = $derived(describe ? describe(data, { format, locale, words }) : null);
  const loading = $derived(state.loading && state.value === undefined);
  const empty = $derived(!loading && !state.error && (described ? described.empty : !hasPoints(data.series)));
  const summary = $derived(label ?? (described ? described.summary : summaryOf(kind, data.series, data.categories, format, locale, words)));
  const table = $derived(described ? described.table : tableOf(kind, data.series, data.categories, format, locale));
  const px = $derived(typeof height === "number" ? `${height}px` : height);

  let element = $state(null);
  let canvas = $state(null);
  let chart = null;
  /** The instance was just given its option: the next update has nothing new. */
  let fresh = false;
  let look = $state(0);
  let at = $state(-1);

  const option = () => build(data, { ...props, format, locale }, themeOf(element));

  onMount(() => {
    const stop = watchTheme(() => look++);
    return () => {
      stop();
      chart?.dispose();
      chart = null;
    };
  });

  // The chart, once there's somewhere to draw it; again if the renderer changes.
  $effect(() => {
    if (!canvas || unavailable) return;
    echarts.use(use);
    const made = echarts.init(canvas, null, { renderer });
    chart = made;
    made.on?.("click", (p) => onselect?.({ series: p.seriesName, category: p.name, value: p.value }));
    const resize = typeof ResizeObserver === "function" ? new ResizeObserver(() => made.resize()) : null;
    resize?.observe(canvas);
    untrack(() => draw(made));
    fresh = true;
    // A chart that moves (RFC 0059) starts once it's drawn, and stops with it.
    const stopMotion = untrack(() => onchart?.(made));
    return () => {
      stopMotion?.();
      resize?.disconnect();
      made.dispose();
      if (chart === made) chart = null;
    };
  });

  // New data, or a new look: the chart moves to it rather than starting over.
  $effect(() => {
    void data;
    void look;
    void locale;
    void props;
    if (!chart) return;
    if (fresh) {
      fresh = false;
      return;
    }
    draw(chart, { replaceMerge: ["series"] });
  });

  const count = $derived(
    described
      ? described.count
      : kind === "pie"
        ? (data.series[0]?.data.length ?? 0)
        : Math.max(data.categories.length, ...data.series.map((s) => s.data.length), 0),
  );

  // --- Taking the chart out of the app (RFC 0060) ---

  /** The chart's data with plain numbers, for a spreadsheet: no grouping, the locale's decimal sign. */
  function plainTable() {
    const plain = new Intl.NumberFormat(locale, { useGrouping: false, maximumFractionDigits: 20 });
    const say = (v) => (typeof v === "number" ? plain.format(v) : v === null || v === undefined ? "" : String(v));
    return described
      ? describe(data, { format: say, locale, words }).table
      : tableOf(kind, data.series, data.categories, say, locale);
  }

  const background = () => themeOf(element).surface;
  const nameFor = (extension) => fileNameOf({ label, summary, extension });
  /** Whether this chart is drawn as SVG, which it can then give as such. */
  const asSvg = () => renderer === "svg" && !unavailable;

  async function image(format) {
    if (!chart) throw new Error(words.notDrawn);
    if (format === "svg") return svgOf(chart, background());
    if (asSvg()) return pngFromSvg(svgOf(chart, background()), chart.getWidth(), chart.getHeight());
    return pngOfCanvas(chart, background());
  }

  /**
   * Save the chart as `png` or `svg`, or its data as `csv`, where the user
   * chooses. The path written, or null if they cancelled.
   */
  export async function save(format = "png") {
    const filters = { png: "PNG", svg: "SVG", csv: "CSV" };
    const data = format === "csv" ? csvOf(plainTable(), { caption: label ?? summary, head: words.category, locale }) : await image(format);
    return files.save(data, {
      defaultName: nameFor(format),
      filters: [{ name: filters[format] ?? format, extensions: [format] }],
    });
  }

  /** Put the chart on the clipboard as an image. */
  export async function copy() {
    await clipboard.writeImage(await image("png"));
  }

  /** The chart's data as CSV text, in the app's locale. */
  export function csv() {
    return csvOf(plainTable(), { caption: label ?? summary, head: words.category, locale });
  }

  /** Whether the shell can save files and hold images: asked once, by the page. */
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

  async function openMenu(event) {
    if (!exportable || empty || loading) return;
    event.preventDefault();
    const at = event;
    const full = await hasSystem();
    const items = [];
    if (full) {
      items.push({ label: words.copyImage, action: () => attempt(copy, words.copied) });
      items.push({ label: words.savePng, action: () => attempt(() => save("png")) });
      if (asSvg()) items.push({ label: words.saveSvg, action: () => attempt(() => save("svg")) });
      items.push({ separator: true });
    }
    items.push({
      label: words.copyData,
      action: () =>
        attempt(async () => {
          if (full) await clipboard.writeText(csv());
          else await navigator.clipboard.writeText(csv());
        }, words.copied),
    });
    if (full) items.push({ label: words.saveCsv, action: () => attempt(() => save("csv")) });
    contextMenu(at, items);
  }

  function onkeydown(event) {
    // Shift+F10 or the Menu key: the context menu, at the chart's corner.
    if ((event.key === "F10" && event.shiftKey) || event.key === "ContextMenu") {
      const rect = event.currentTarget.getBoundingClientRect();
      void openMenu(new MouseEvent("contextmenu", { clientX: rect.left + 16, clientY: rect.top + 16, cancelable: true }));
      event.preventDefault();
      return;
    }
    if (!chart || count === 0) return;
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      at = at < 0 ? 0 : (at + (event.key === "ArrowRight" ? 1 : -1) + count) % count;
      chart.dispatchAction({ type: "showTip", seriesIndex: 0, dataIndex: at });
    } else if (event.key === "Escape") {
      at = -1;
      chart.dispatchAction({ type: "hideTip" });
    } else if (event.key === "Enter" && at >= 0 && onselect && described) {
      const chosen = described.select(at);
      if (chosen) onselect(chosen);
    } else if (event.key === "Enter" && at >= 0 && onselect) {
      const first = data.series[0];
      const point = first?.data[at];
      const category = kind === "pie" ? (point?.name ?? data.categories[at]) : data.categories[at];
      onselect({ series: first?.name, category, value: valueOf(point) });
    }
  }
</script>

<div class="elyra-chart {className}" style:--height={px} bind:this={element}>
  {#if loading}
    <Skeleton height={px} />
  {:else if state.error || drawError}
    <Callout variant="danger" title={words.failed}>{(state.error ?? drawError)?.message ?? String(state.error ?? drawError)}</Callout>
  {:else if empty}
    <Empty title={words.empty} />
  {:else}
    <!-- One stop in the tab order: ←/→ walk the categories, Enter selects. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
    <div
      class="frame"
      role="group"
      aria-roledescription="chart"
      aria-label={summary}
      tabindex="0"
      {onkeydown}
      oncontextmenu={openMenu}
      onblur={() => chart?.dispatchAction({ type: "hideTip" })}
    >
      {#if unavailable}
        <Callout variant="info">{unavailable}</Callout>
      {:else}
        <div class="canvas" bind:this={canvas} aria-hidden="true"></div>
      {/if}
      <!-- A table ignores height: the box around it is what hides it. -->
      <div class="data">
        <table>
          <caption>{words.caption}</caption>
          <thead>
            <tr>
              <th scope="col">{table.head ?? words.category}</th>
              {#each table.columns as column, i (i)}<th scope="col">{column}</th>{/each}
            </tr>
          </thead>
          <tbody>
            {#each table.rows as [name, values], r (r)}
              <tr>
                <th scope="row">{name}</th>
                {#each values as value, i (i)}<td>{value}</td>{/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      {@render children?.()}
    </div>
  {/if}
</div>

<style>
  .elyra-chart { position: relative; width: 100%; font: var(--elyra-font-size) var(--elyra-font); color: var(--elyra-text); }
  .frame { border-radius: var(--elyra-radius); outline: none; }
  .frame:focus-visible { box-shadow: 0 0 0 2px var(--elyra-focus); }
  .canvas { width: 100%; height: var(--height); }
  /* Read, not seen: the screen reader's and the assistant's. */
  .data {
    position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
    overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; border: 0;
  }
</style>
