<!--
  A WireCharts chart (RFC 0059): its props to WireCharts' builder (ported to
  JavaScript, held to WireCharts by fixtures), the app's look over the
  option, and the rest of ChartBase: a live query's states, the summary and
  the table read from the option, the keys.
-->
<script>
  import ChartBase from "./ChartBase.svelte";
  import { applyTheme } from "./wirecharts/theme.js";
  import { describeOption } from "./describe.js";
  import { clock, play, race } from "./wirecharts/motion.js";
  import { WORDS } from "./words.js";
  import { useWords } from "@elyra/ui/internal/words.js";

  let {
    slug,
    call,
    gl = false,
    use = [],
    props = {},
    live = undefined,
    height = 320,
    label = undefined,
    format = undefined,
    locale = undefined,
    renderer = "svg",
    onselect = undefined,
    words = {},
    exportable = true,
    class: className = "",
  } = $props();

  // Taking it out of the app (RFC 0060): for an app's own button.
  let base;
  export const save = (format) => base.save(format);
  export const copy = () => base.copy();
  export const csv = () => base.csv();


  /** A live value over the props: a list is the series, an object its props. */
  const withLive = (value) =>
    value === undefined || value === null
      ? props
      : Array.isArray(value)
        ? { ...props, series: value }
        : { ...props, ...value };

  // Its own call only, so an app bundles only the builders it draws with.
  const option = (data) => call(withLive(data.value));
  const build = (data, _p, theme) => applyTheme(option(data), theme);
  const describe = (data, ctx) => describeOption(option(data), ctx);

  // 3D needs WebGL; without it the chart says so, and still has its summary and table.
  const webgl = () => {
    try {
      return !!document.createElement("canvas").getContext("webgl");
    } catch {
      return false;
    }
  };
  const unavailable = gl && typeof document !== "undefined" && !webgl();

  // The ones that move (RFC 0059).
  const said = useWords("charts", () => words, WORDS);
  // Not state: Svelte would proxy it, and ECharts must get its own instance.
  let drawn = null;
  let racing = null;
  let playing = $state(null);
  const MOVES = new Set(["clock", "line-race", "area-race", "audio"]);
  const onchart = MOVES.has(slug)
    ? (chart) => {
        drawn = chart;
        if (slug === "clock") return clock(chart);
        if (slug === "line-race" || slug === "area-race") {
          racing = race(chart, option({ value: undefined }), props.interval ?? 400);
          return racing.stop;
        }
        return () => playing?.();
      }
    : undefined;
  function toggleSound() {
    if (playing) return playing();
    playing = play(drawn, option({ value: undefined }), props, () => (playing = null));
  }
</script>

<ChartBase
  bind:this={base}
  {exportable}
  kind={slug}
  {use}
  {build}
  {describe}
  {live}
  {height}
  {label}
  {format}
  {locale}
  {renderer}
  {onselect}
  labels={words}
  class={className}
  {props}
  {onchart}
  unavailable={unavailable ? said.noGl : null}
>
  {#if slug === "line-race" || slug === "area-race"}
    <button type="button" class="elyra-chart-control" onclick={() => racing?.play()}>{said.replay}</button>
  {:else if slug === "audio"}
    <button type="button" class="elyra-chart-control" aria-pressed={playing !== null} onclick={toggleSound}>
      {playing ? said.stop : said.play}
    </button>
  {/if}
</ChartBase>

<style>
  .elyra-chart-control {
    margin-top: 8px; padding: 4px 10px; border-radius: var(--elyra-radius);
    border: 1px solid var(--elyra-border); background: var(--elyra-raised); color: var(--elyra-text);
    font: 13px var(--elyra-font); cursor: pointer;
  }
  .elyra-chart-control:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
</style>
