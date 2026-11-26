<!--
  Points on two value axes, `[x, y]`; with `size`, a third value is each bubble's size.
  The data is WireCharts': `series` (`[{ name, data }]`, or a list of
  numbers) and `categories`, or a live query's value (RFC 0058).
-->
<script>
  import { ScatterChart as Drawn } from "echarts/charts";
  import ChartBase from "./internal/ChartBase.svelte";
  import { scatter } from "./internal/options.js";

  let {
    series = [],
    categories = [],
    live = undefined,
    size = false,
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

  const build = (d, p, t) => scatter(d, p, t);
</script>

<ChartBase
  kind="scatter"
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
  props={{ size, legend, tooltip }}
/>
