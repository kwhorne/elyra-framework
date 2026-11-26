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
  } = $props();

  // What every chart draws with; each chart adds its own part (`use`).
  echarts.use([SVGRenderer, CanvasRenderer, AriaComponent, GridComponent, LegendComponent, TooltipComponent]);

  const words = useWords("charts", () => labels, WORDS);
  const appLocale = useAppLocale();
  const locale = $derived(localeProp ?? appLocale());

  /** `{ value, error, loading }`, from a live store, a `Live`, or a plain value. */
  let state = $state({ value: undefined, error: null, loading: false });
  $effect(() => {
    if (live?.subscribe) return live.subscribe((s) => (state = s ?? { value: undefined, error: null, loading: false }));
    state = live === undefined ? { value: undefined, error: null, loading: false } : { value: live, error: null, loading: false };
  });

  const data = $derived(chartData({ series, categories }, state.value));
  const loading = $derived(state.loading && state.value === undefined);
  const empty = $derived(!loading && !state.error && !hasPoints(data.series));
  const summary = $derived(label ?? summaryOf(kind, data.series, data.categories, format, locale, words));
  const table = $derived(tableOf(kind, data.series, data.categories, format, locale));
  const px = $derived(typeof height === "number" ? `${height}px` : height);

  let element = $state(null);
  let canvas = $state(null);
  let chart = null;
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
    if (!canvas) return;
    echarts.use(use);
    const made = echarts.init(canvas, null, { renderer });
    chart = made;
    made.on?.("click", (p) => onselect?.({ series: p.seriesName, category: p.name, value: p.value }));
    const resize = typeof ResizeObserver === "function" ? new ResizeObserver(() => made.resize()) : null;
    resize?.observe(canvas);
    untrack(() => made.setOption(option()));
    return () => {
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
    if (chart) chart.setOption(option(), { replaceMerge: ["series"] });
  });

  const count = $derived(kind === "pie" ? (data.series[0]?.data.length ?? 0) : Math.max(data.categories.length, ...data.series.map((s) => s.data.length), 0));

  function onkeydown(event) {
    if (!chart || count === 0) return;
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      at = at < 0 ? 0 : (at + (event.key === "ArrowRight" ? 1 : -1) + count) % count;
      chart.dispatchAction({ type: "showTip", seriesIndex: 0, dataIndex: at });
    } else if (event.key === "Escape") {
      at = -1;
      chart.dispatchAction({ type: "hideTip" });
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
  {:else if state.error}
    <Callout variant="danger" title={words.failed}>{state.error?.message ?? String(state.error)}</Callout>
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
      onblur={() => chart?.dispatchAction({ type: "hideTip" })}
    >
      <div class="canvas" bind:this={canvas} aria-hidden="true"></div>
      <table class="data">
        <caption>{words.caption}</caption>
        <thead>
          <tr>
            <th scope="col">{words.category}</th>
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
