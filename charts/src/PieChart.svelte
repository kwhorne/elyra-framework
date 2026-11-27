<!--
  Shares of a whole: the first series' points (`{ name, value }`, or numbers named by `categories`).
  `donut` leaves the middle open; `rose` sizes each slice by its value.
  WireCharts' `labels` (a list) names the slices as `categories` does (RFC 0059).
  The data is WireCharts': `series` (`[{ name, data }]`, or a list of
  numbers) and `categories`, or a live query's value (RFC 0058).
-->
<script>
  import { PieChart as Drawn } from "echarts/charts";
  import ChartBase from "./internal/ChartBase.svelte";
  import { pie } from "./internal/options.js";

  let {
    series = [],
    categories = [],
    live = undefined,
    donut = false,
    rose = false,
    height = 320,
    legend = true,
    tooltip = true,
    label = undefined,
    format = undefined,
    locale = undefined,
    renderer = "svg",
    onselect = undefined,
    exportable = true,
    labels = {},
    class: className = "",
  } = $props();

  // Taking it out of the app (RFC 0060): for an app's own button.
  let base;
  export const save = (format) => base.save(format);
  export const copy = () => base.copy();
  export const csv = () => base.csv();

  const build = (d, p, t) => pie(d, p, t);
  // A list is WireCharts' names for the slices; an object, the chart's words.
  const names = $derived(Array.isArray(labels) ? labels : null);
  const words = $derived(Array.isArray(labels) ? {} : labels);
</script>

<ChartBase
  bind:this={base}
  {exportable}
  kind="pie"
  use={[Drawn]}
  {build}
  {series}
  categories={names ?? categories}
  {live}
  {height}
  {label}
  {format}
  {locale}
  {renderer}
  {onselect}
  labels={words}
  class={className}
  props={{ donut, rose, legend, tooltip }}
/>
