<!--
  What every chart is (RFC 0058): ECharts in an element of its own, the
  option rebuilt from the data and the theme; a live query's states; a
  summary as its name and its data as a table the screen reader and the
  assistant read; ←/→ along the categories, Enter to select. With a data
  zoom (RFC 0061), the window: `bind:range`, the keys, the slider's handles
  for the keyboard, and what's shown in what it says. Linked by a name
  (RFC 0063), charts share their window and their pointer; with `brush`,
  the user selects, by the pointer or the keys, and `selection` is it.
  Marks (RFC 0065) are said after what's shown, and beside the table.
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
    nearestIndex,
    snapRange,
  } from "./zoom.js";
  import { join, others } from "./link.js";
  import {
    areasOf,
    brushTypeOf,
    pointAt,
    pointCount,
    sameSelection,
    selectedBy,
    selectionOf,
    spanOf,
    withBrush,
  } from "./brush.js";
  import { bandRows, marksSaid } from "./marks.js";
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
    link = undefined,
    brush = false,
    selection = $bindable(null),
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
      // The pointer's axis (RFC 0063) is read again, once, after a drawing.
      drawnAxis = null;
      if (brush) {
        const scatter = kind === "scatter";
        // A span needs its axis, zoomed or not.
        const along = axis ?? (scatter ? null : axisOf(drawn));
        drawn = withBrush(drawn, themeOf(element), scatter, along);
        brushAxis = along;
        lastOption = drawn;
      }
      chart.setOption(drawn, opts);
      if (brush) untrack(() => armBrush(chart));
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
  // A selection (RFC 0063): said after what's shown.
  const chosen = $derived.by(() => {
    if (!selection) return null;
    if (selection.points) return words.selected(selection.points.length, pointCount(data.series));
    return words.selectedSpan(textAt(selection.from), textAt(selection.to), selection.categories?.length ?? 0);
  });
  // Marks (RFC 0065): a sentence each, over what's shown; null for one outside the window.
  const marked = $derived.by(() => {
    if (described || !props.marks) return [];
    const scatter = kind === "scatter";
    const windowed = Boolean(ends);
    const xIn = (x) => !windowed || (x >= range[0] && x <= range[1]);
    return marksSaid(props.marks, {
      kind,
      series: data.series,
      categories: data.categories,
      stack: props.stack,
      percent: props.percent,
      format,
      locale,
      words,
      axis: windowed ? axis : null,
      range: windowed ? range : null,
      shown: scatter ? xIn : shownBy(windowed ? axis : null, windowed ? range : null),
    });
  });
  /** The table's rows each category band covers, pointed at its sentence. */
  const bands = $derived(described || kind === "scatter" || !props.marks ? [] : bandRows(props.marks, { categories: data.categories, locale }));
  const uid = $props.id();
  const markId = (i) => `${uid}-mark-${i}`;
  const bandsOf = (r) => bands.filter(([i, rows]) => marked[i] && rows.has(r)).map(([i]) => markId(i)).join(" ") || undefined;
  const said = $derived(ends ? words.window(ends[0], ends[1], shownSummary) : whole);
  const summary = $derived(label ?? [said, ...marked.filter(Boolean), chosen].filter(Boolean).join(". "));
  const table = $derived(described ? described.table : tableOf(kind, data.series, data.categories, format, locale));
  /** Whether the table's `i`th row is in the window: all rows are kept, the shown ones marked. */
  const isShown = $derived(shownBy(axis, range));
  /** Whether the table's `i`th row is selected (RFC 0063). */
  const isSelected = $derived(selectedBy(selection, brushAxis ?? axis, data.series, kind === "scatter"));
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
    made.on?.("updateAxisPointer", (p) => pointed(p));
    made.on?.("globalout", () => pointedAway());
    made.on?.("brushSelected", (p) => brushed(p));
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
    following = true;
    chart.dispatchAction({ type: "dataZoom", start: next.start, end: next.end });
    following = false;
    settle();
    share();
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
    if (!following) share();
  }

  // The app set the window.
  $effect(() => {
    const wanted = range;
    untrack(() => {
      if (!chart || !axis || sameRange(wanted, rangeOf(axis, win))) return;
      win = windowOf(axis, wanted);
      following = true;
      chart.dispatchAction({ type: "dataZoom", start: win.start, end: win.end });
      following = false;
      share();
    });
  });

  // --- Linked charts (RFC 0063) ---

  /** A window this chart was given (by the app, or a linked chart) isn't passed on again by its echo. */
  let following = false;
  /** The pointer a linked chart put here: its echo isn't passed back. */
  let quietUntil = 0;
  let lastPointed;
  let linkedWith = $state(0);
  const me = {
    follow(other) {
      if (!chart || !axis) return;
      const wanted = other ? snapRange(axis, other) : null;
      if (sameRange(wanted, rangeOf(axis, win))) return;
      win = windowOf(axis, wanted);
      following = true;
      chart.dispatchAction({ type: "dataZoom", start: win.start, end: win.end });
      following = false;
      settle();
    },
    point(value) {
      const i = nearestIndex(pointerAxis(), value);
      if (!chart || i < 0) return;
      quietUntil = Date.now() + 120;
      chart.dispatchAction({ type: "showTip", seriesIndex: 0, dataIndex: i });
    },
    unpoint() {
      quietUntil = Date.now() + 120;
      chart?.dispatchAction({ type: "hideTip" });
    },
    changed() {
      linkedWith = link ? others(link, me).length : 0;
    },
  };
  $effect(() => {
    if (!link) return;
    const leave = join(link, me);
    return () => {
      leave();
      linkedWith = 0;
    };
  });

  /** The axis the pointer is on: the window's, or the drawn option's. */
  /** The drawn option's axis, read when a linked chart first needs it after a drawing. */
  let drawnAxis = null;
  const pointerAxis = () => axis ?? brushAxis ?? (drawnAxis ??= chart ? axisOf(option()) : null);

  function share() {
    if (!link) return;
    for (const other of others(link, me)) other.follow(range);
  }

  /** The pointer moved over this chart: the linked ones point at the same value. */
  function pointed(params) {
    if (!link || Date.now() < quietUntil) return;
    const on = pointerAxis();
    if (!on) return;
    const dim = on.vertical ? "y" : "x";
    const info = (params?.axesInfo ?? []).find((a) => a.axisDim === dim) ?? params?.axesInfo?.[0];
    if (!info) return;
    const value = on.type === "category" && typeof info.value === "number" ? on.values[info.value] : info.value;
    if (value === undefined || value === lastPointed) return;
    lastPointed = value;
    for (const other of others(link, me)) other.point(value);
  }

  function pointedAway() {
    if (!link || Date.now() < quietUntil) return;
    lastPointed = undefined;
    for (const other of others(link, me)) other.unpoint();
  }

  // --- A selection (RFC 0063) ---

  /** The axis a span is along (set when drawn), and the option drawn, for the brush's indexes. */
  let brushAxis = $state.raw(null);
  let lastOption = null;
  /** The selection as the chart last made it: the app's own is drawn, not this. */
  let ours = null;
  /** Where Space started a span, by the keys. */
  let anchor = null;
  let lasso = false;
  let announced = $state("");

  /** The brush is the pointer's tool while the chart has `brush`. */
  function armBrush(target) {
    target.dispatchAction({
      type: "takeGlobalCursor",
      key: "brush",
      brushOption: { brushType: brushTypeOf(kind === "scatter", brushAxis, lasso), brushMode: "single" },
    });
  }

  function brushed(params) {
    if (!brush) return;
    const next = selectionOf(params, lastOption, brushAxis, kind === "scatter");
    if (sameSelection(next, selection)) return;
    ours = next;
    selection = next;
  }

  // The app set the selection: draw it.
  $effect(() => {
    const wanted = selection;
    untrack(() => {
      if (!chart || !brush || wanted === ours) return;
      ours = wanted;
      chart.dispatchAction({ type: "brush", areas: areasOf(wanted, brushAxis, kind === "scatter") });
    });
  });

  // Shift over a scatter: the lasso.
  $effect(() => {
    if (!brush || kind !== "scatter" || typeof window === "undefined") return;
    const shift = (event) => {
      if (event.key !== "Shift" || lasso === (event.type === "keydown")) return;
      lasso = event.type === "keydown";
      if (chart) armBrush(chart);
    };
    window.addEventListener("keydown", shift);
    window.addEventListener("keyup", shift);
    return () => {
      window.removeEventListener("keydown", shift);
      window.removeEventListener("keyup", shift);
    };
  });

  const textAt = (v) => textOf(brushAxis ?? axis, v, format, locale);

  /** Space, and the arrows after it, and Esc: a selection by the keys. True if it was one of them. */
  function selectKeys(event) {
    if (!brush || !chart) return false;
    const scatter = kind === "scatter";
    if (event.key === " ") {
      event.preventDefault();
      const here = at < 0 ? 0 : at;
      if (scatter) {
        const point = pointAt(lastOption?.series ?? [], 0, here);
        if (!point) return true;
        const points = selection?.points ?? [];
        const has = points.some((p) => p.seriesIndex === 0 && p.index === here);
        const next = has ? points.filter((p) => !(p.seriesIndex === 0 && p.index === here)) : [...points, point];
        chooseByKeys(next.length ? { points: next } : null);
        chart.dispatchAction({ type: has ? "unselect" : "select", seriesIndex: 0, dataIndex: here });
      } else if (anchor === null) {
        anchor = here;
        chooseByKeys(spanOf(brushAxis, here, here));
      } else {
        anchor = null;
      }
      return true;
    }
    if (event.key === "Escape" && selection) {
      anchor = null;
      chooseByKeys(null);
      chart.dispatchAction({ type: "brush", areas: [] });
      if (scatter) chart.dispatchAction({ type: "unselect", seriesIndex: 0, dataIndex: (lastOption?.series?.[0]?.data ?? []).map((_, i) => i) });
      return false;
    }
    return false;
  }

  /** After ←/→ moved `at`: a span the keys are making grows to it. */
  function extendByKeys() {
    if (!brush || anchor === null || kind === "scatter" || !brushAxis) return;
    const span = spanOf(brushAxis, anchor, at);
    chooseByKeys(span);
    chart.dispatchAction({ type: "brush", areas: areasOf(span, brushAxis, false) });
  }

  function chooseByKeys(next) {
    ours = next;
    selection = next;
    announced = next
      ? next.points
        ? words.selected(next.points.length, pointCount(data.series))
        : words.selectedSpan(textAt(next.from), textAt(next.to), next.categories.length)
      : words.selectionCleared;
  }

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
    if (selectKeys(event)) return;
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      // Zoomed in, the walk starts in the window, and the window follows it.
      const first = range ? Math.max(0, Array.from({ length: count }, (_, i) => i).find((i) => isShown(i)) ?? 0) : 0;
      at = at < 0 ? first : (at + (event.key === "ArrowRight" ? 1 : -1) + count) % count;
      if (axis && !isAll(win)) move(keepInView(win, percentAt(axis, at)));
      chart.dispatchAction({ type: "showTip", seriesIndex: 0, dataIndex: at });
      extendByKeys();
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
      aria-description={linkedWith ? words.linked(linkedWith) : undefined}
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
                <th scope="row" aria-current={ends && isShown(r) ? "true" : undefined} aria-describedby={bandsOf(r)}
                  >{name}{#if isSelected(r)}<span class="said">, {words.rowSelected}</span>{/if}</th
                >
                {#each values as value, i (i)}<td>{value}</td>{/each}
              </tr>
            {/each}
          </tbody>
        </table>
        {#each marked as sentence, i (i)}{#if sentence}<p id={markId(i)}>{sentence}</p>{/if}{/each}
      </div>
      <!-- What the keys select, said as it changes (RFC 0063). -->
      {#if brush}<div class="said" role="status" aria-live="polite">{announced}</div>{/if}
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
  .said,
  .data {
    position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px;
    overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; border: 0;
  }
</style>
