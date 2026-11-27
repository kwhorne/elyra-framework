<!--
  Values over the categories, as a line: `smooth`, `step`, `stack`, and `area` to fill under it.
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
    step = false,
    stack = false,
    area = false,
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

  const build = (d, p, t) => cartesian("line", d, p, t);
</script>

<ChartBase
  bind:this={base}
  {exportable}
  kind="line"
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
  props={{ smooth, step, stack, area, legend, tooltip }}
/>
