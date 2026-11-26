<!--
  A line filled to the axis: `stack` them, or `percent` for each category's shares.
  The data is WireCharts': `series` (`[{ name, data }]`, or a list of
  numbers) and `categories`, or a live query's value (RFC 0058).
-->
<script>
  import { LineChart as Drawn } from "echarts/charts";
  import ChartBase from "./internal/ChartBase.svelte";
  import { cartesian } from "./internal/options.js";

  let {
    series = [],
    categories = [],
    live = undefined,
    smooth = false,
    stack = false,
    percent = false,
    height = 320,
    legend = true,
    tooltip = true,
    label = undefined,
    format = undefined,
    locale = undefined,
    renderer = "svg",
    onselect = undefined,
    labels = {},
    class: className = "",
  } = $props();

  const build = (d, p, t) => cartesian("area", d, p, t);
</script>

<ChartBase
  kind="area"
  use={[Drawn]}
  {build}
  {series}
  {categories}
  {live}
  {height}
  {label}
  {format}
  {locale}
  {renderer}
  {onselect}
  {labels}
  class={className}
  props={{ smooth, stack, percent, legend, tooltip }}
/>
