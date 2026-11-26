<!--
  Bars across, the categories down the side (long names fit): `stack` or `percent`.
  The data is WireCharts': `series` (`[{ name, data }]`, or a list of
  numbers) and `categories`, or a live query's value (RFC 0058).
-->
<script>
  import { BarChart as Drawn } from "echarts/charts";
  import ChartBase from "./internal/ChartBase.svelte";
  import { cartesian } from "./internal/options.js";

  let {
    series = [],
    categories = [],
    live = undefined,
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

  const build = (d, p, t) => cartesian("bar", d, p, t);
</script>

<ChartBase
  kind="bar"
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
  props={{ stack, percent, legend, tooltip }}
/>
