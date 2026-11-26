<!--
  Shares of a whole: the first series' points (`{ name, value }`, or numbers named by `categories`).
  `donut` leaves the middle open; `rose` sizes each slice by its value.
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
    labels = {},
    class: className = "",
  } = $props();

  const build = (d, p, t) => pie(d, p, t);
</script>

<ChartBase
  kind="pie"
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
  props={{ donut, rose, legend, tooltip }}
/>
