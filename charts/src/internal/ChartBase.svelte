<!--
  What every chart is (RFC 0058): ECharts in an element of its own, the
  option rebuilt from the data and the theme; a live query's states; a
  summary as its name and its data as a table the screen reader and the
  assistant read; ←/→ along the categories, Enter to select. With a data
  zoom (RFC 0061), the window: `bind:range`, the keys, the slider's handles
  for the keyboard, and what's shown in what it says.
-->
<script>
  import { onMount, untrack } from "svelte";
  import * as echarts from "echarts/core";
  import { SVGRenderer, CanvasRenderer } from "echarts/renderers";
  import { AriaComponent, GridComponent, LegendComponent, TooltipComponent } from "echarts/components";
  import { Callout, Empty, Skeleton } from "@elyra/ui";
  import { clipboard, contextMenu, files, paths, store, toast } from "@elyra/runtime";
  import { csvOf, fileNameOf, pngFromSvg, pngOfCanvas, svgOf } from "./export.js";
  import { useAppLocale, useWords } from "@elyra/ui/internal/words.js";
  import { chartData, hasPoints, summaryOf, tableOf, valueOf } from "./data.js";
  import {
    axisOf,
    follow,
    isAll,
    keepInView,
    panBy,
    percentAt,
    rangeOf,
    sameRange,
    shownBy,
    sliceData,
    sliceOption,
    stepOf,
    styleZoom,
    textOf,
    windowOf,
    zoomBy,
    zooms,
  } from "./zoom.js";
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
    range = $bindable(null),
    persist = undefined,
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
      let drawn = option();
      if (zooms(drawn)) {
        drawn = untrack(() => {
          // New data keeps the window: on its values, or at the end if it was there.
          const next = axisOf(drawn);
          win = axis ? follow(axis, next, win, range) : windowOf(next, range);
          axis = next;
          settle();
          return styleZoom(drawn, themeOf(element), next, win);
        });
      } else if (axis) {
        axis = null;
      }
      chart.setOption(drawn, opts);
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
  // Zoom (RFC 0061): the axis the window is on (null without a data zoom), and the window as percents.
  let axis = $state.raw(null);
  let win = $state.raw({ start: 0, end: 100 });
  /** The window's ends, as the chart writes them. */
  const ends = $derived(axis && range ? range.map((v) => textOf(axis, v, format, locale)) : null);
  const whole = $derived(described ? described.summary : summaryOf(kind, data.series, data.categories, format, locale, words));
  const shownSummary = $derived.by(() => {
    if (!ends) return whole;
    if (described) return describe(data, { format, locale, words, window: (o) => sliceOption(o, axis, range) }).summary;
    const shown = sliceData(data, kind, axis, range);
    return summaryOf(kind, shown.series, shown.categories, format, locale, words);
  });
  const summary = $derived(label ?? (ends ? words.window(ends[0], ends[1], shownSummary) : whole));
  const table = $derived(described ? described.table : tableOf(kind, data.series, data.categories, format, locale));
  /** Whether the table's `i`th row is in the window: all rows are kept, the shown ones marked. */
  const isShown = $derived(shownBy(axis, range));
  const px = $derived(typeof height === "number" ? `${height}px` : height);

  let element = $state(null);
  let canvas = $state(null);
  let chart = null;
  /**
   * What the instance was last drawn from. Its first drawing can come after
   * the update below has run (the canvas is bound later), so the update
   * compares, rather than skipping its next run: that dropped the first new
   * value of a chart that had data from the start.
   */
  let drawnFrom = null;
  const inputs = () => [data, look, locale, props];
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
    made.on?.("datazoom", (p) => zoomed(p));
    const resize = typeof ResizeObserver === "function" ? new ResizeObserver(() => made.resize()) : null;
    resize?.observe(canvas);
    untrack(() => {
      draw(made);
      drawnFrom = inputs();
    });
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
    const now = inputs();
    if (!chart) return;
    if (drawnFrom && now.every((v, i) => v === drawnFrom[i])) return;
    drawnFrom = now;
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
  const nameFor = (extension) => fileNameOf({ label, summary: whole, extension });
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
    const data = format === "csv" ? csvOf(plainTable(), { caption: label ?? whole, head: words.category, locale }) : await image(format);
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
    return csvOf(plainTable(), { caption: label ?? whole, head: words.category, locale });
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

  // --- The window (RFC 0061) ---

  /** `range` from the window, when it says something new. */
  function settle() {
    const next = rangeOf(axis, win);
    if (!sameRange(next, range)) range = next;
  }

  /** Show `next`: ECharts moves to it, and `range` follows. */
  function move(next) {
    if (!chart || !axis) return;
    if (next.start === win.start && next.end === win.end) return;
    win = next;
    chart.dispatchAction({ type: "dataZoom", start: next.start, end: next.end });
    settle();
  }

  /** ECharts moved the window: the wheel, a drag, the slider. */
  function zoomed(params) {
    if (!axis) return;
    const at = params?.batch?.[0] ?? params ?? {};
    if (Number.isFinite(at.start) && Number.isFinite(at.end)) {
      win = { start: at.start, end: at.end };
    } else if (at.startValue !== undefined && at.endValue !== undefined) {
      const value = (v) => (axis.type === "category" && typeof v === "number" ? axis.values[v] : v);
      win = windowOf(axis, [value(at.startValue), value(at.endValue)]);
    } else {
      return;
    }
    settle();
  }

  // The app set the window.
  $effect(() => {
    const wanted = range;
    untrack(() => {
      if (!chart || !axis || sameRange(wanted, rangeOf(axis, win))) return;
      win = windowOf(axis, wanted);
      chart.dispatchAction({ type: "dataZoom", start: win.start, end: win.end });
    });
  });

  // Kept in the app's settings store with `persist`, and back the next time.
  const zoomKey = $derived(persist ? `elyra-charts.zoom.${persist}` : null);
  let restored = $state(false);
  $effect(() => {
    if (!zoomKey) return;
    let live = true;
    restored = false;
    Promise.resolve()
      .then(() => store.get(zoomKey))
      .then((saved) => {
        if (live && Array.isArray(saved) && saved.length === 2) range = saved;
      })
      .catch(() => {})
      .finally(() => live && (restored = true));
    return () => (live = false);
  });
  let saving;
  $effect(() => {
    const kept = $state.snapshot(range);
    if (!zoomKey || !restored) return;
    const at = zoomKey;
    clearTimeout(saving);
    saving = setTimeout(() => Promise.resolve().then(() => store.set(at, kept)).catch(() => {}), 300);
  });
  $effect(() => () => clearTimeout(saving));

  /** +/-, 0 and Shift+←/→ on the chart: true if it was one of them. */
  function zoomKeys(event) {
    if (!axis) return false;
    const focus = at >= 0 ? percentAt(axis, at) : undefined;
    const width = win.end - win.start;
    if (event.key === "+" || event.key === "=") move(zoomBy(axis, win, 0.5, focus));
    else if (event.key === "-" || event.key === "_") move(zoomBy(axis, win, 2, focus));
    else if (event.key === "0") move({ start: 0, end: 100 });
    else if (event.shiftKey && (event.key === "ArrowRight" || event.key === "ArrowLeft")) {
      const by = Math.max(stepOf(axis), width / 10);
      move(panBy(win, event.key === "ArrowRight" ? by : -by));
    } else return false;
    event.preventDefault();
    return true;
  }

  /** A slider handle's keys: ←/→ (and ↑/↓) a step, Page Up/Down a tenth, Home/End the axis's ends. */
  function handleKeys(event, end) {
    const step = stepOf(axis);
    const big = Math.max(step, 10);
    const moves = { ArrowRight: step, ArrowUp: step, ArrowLeft: -step, ArrowDown: -step, PageUp: big, PageDown: -big, Home: -100, End: 100 };
    const by = moves[event.key];
    if (by === undefined) return;
    event.preventDefault();
    event.stopPropagation();
    const least = axis.type === "category" ? step : 1;
    if (end === "start") move({ start: Math.min(Math.max(0, win.start + by), win.end - least), end: win.end });
    else move({ start: win.start, end: Math.max(Math.min(100, win.end + by), win.start + least) });
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
    if (zoomKeys(event)) return;
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      // Zoomed in, the walk starts in the window, and the window follows it.
      const first = range ? Math.max(0, Array.from({ length: count }, (_, i) => i).find((i) => isShown(i)) ?? 0) : 0;
      at = at < 0 ? first : (at + (event.key === "ArrowRight" ? 1 : -1) + count) % count;
      if (axis && !isAll(win)) move(keepInView(win, percentAt(axis, at)));
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
        {#if axis}
          <!-- ECharts' slider handles aren't in the tab order: these are, over them. -->
          <div class="window" class:vertical={axis.vertical}>
            {#each ["start", "end"] as end (end)}
              <div
                class="handle"
                role="slider"
                tabindex="0"
                aria-label={end === "start" ? words.windowStart : words.windowEnd}
                aria-orientation={axis.vertical ? "vertical" : "horizontal"}
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow={Math.round(end === "start" ? win.start : win.end)}
                aria-valuetext={textOf(axis, (rangeOf(axis, win) ?? [axis.values[0], axis.values.at(-1)])[end === "start" ? 0 : 1], format, locale)}
                style:--at={end === "start" ? win.start : win.end}
                onkeydown={(event) => handleKeys(event, end)}
              ></div>
            {/each}
          </div>
        {/if}
      {/if}
      <!-- A table ignores height: the box around it is what hides it. -->
      <div class="data">
        <table>
          <caption>{ends ? words.shownCaption(ends[0], ends[1]) : words.caption}</caption>
          <thead>
            <tr>
              <th scope="col">{table.head ?? words.category}</th>
              {#each table.columns as column, i (i)}<th scope="col">{column}</th>{/each}
            </tr>
          </thead>
          <tbody>
            {#each table.rows as [name, values], r (r)}
              <tr>
                <th scope="row" aria-current={ends && isShown(r) ? "true" : undefined}>{name}</th>
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
  .frame { position: relative; border-radius: var(--elyra-radius); outline: none; }
  /* Over ECharts' slider (zoom.js's SLIDER): the pointer goes through to it. */
  .window { position: absolute; left: 48px; right: 48px; bottom: 8px; height: 22px; pointer-events: none; }
  .window.vertical { left: auto; right: 8px; top: 16px; bottom: 16px; width: 22px; height: auto; }
  .handle {
    position: absolute; top: 0; bottom: 0; width: 12px; left: calc(var(--at) * 1%);
    transform: translateX(-50%); border-radius: var(--elyra-radius); outline: none;
  }
  .vertical .handle { left: 0; right: 0; width: auto; height: 12px; top: calc(var(--at) * 1%); bottom: auto; transform: translateY(-50%); }
  .handle:focus-visible { box-shadow: 0 0 0 2px var(--elyra-focus); }
  .frame:focus-visible { box-shadow: 0 0 0 2px var(--elyra-focus); }
  .canvas { width: 100%; height: var(--height); }
  /* Read, not seen: the screen reader's and the assistant's. */
  .data {
    position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
    overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; border: 0;
  }
</style>
