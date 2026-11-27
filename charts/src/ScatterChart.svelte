<!--
  Points on two value axes, `[x, y]`; with `size`, a third value is each bubble's size.
  The data is WireCharts': `series` (`[{ name, data }]`, or a list of
  numbers) and `categories`, or a live query's value (RFC 0058).
-->
<script>
  import { ScatterChart as Drawn } from "echarts/charts";
  import { DataZoomComponent } from "echarts/components";
  import ChartBase from "./internal/ChartBase.svelte";
  import { scatter } from "./internal/options.js";

  let {
    series = [],
    categories = [],
    live = undefined,
    size = false,
    zoom = false,
    range = $bindable(null),
    persist = undefined,
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

  const build = (d, p, t) => scatter(d, p, t);
</script>

<ChartBase
  bind:this={base}
  bind:range
  {persist}
  {exportable}
  kind="scatter"
  use={zoom ? [Drawn, DataZoomComponent] : [Drawn]}
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
  props={{ size, legend, tooltip, zoom }}
/>
