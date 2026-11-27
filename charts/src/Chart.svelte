<!--
  Anything ECharts draws: `option` over the theme's (colours, text, the
  tooltip), and `use`, the ECharts parts it needs (`[HeatmapChart,
  VisualMapComponent]` from `echarts/charts` and `echarts/components`), so
  only those are in the app (RFC 0058). `series` and `categories` are for
  the summary and the table; the option draws.
-->
<script>
  import ChartBase from "./internal/ChartBase.svelte";
  import { custom } from "./internal/options.js";

  let {
    option = {},
    use = [],
    kind = "line",
    series = undefined,
    categories = [],
    live = undefined,
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

  const build = (_d, p, t) => custom(option, p, t);
</script>

<ChartBase
  bind:this={base}
  {exportable}
  {kind}
  {use}
  {build}
  series={series ?? option.series ?? []}
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
  props={{ legend, tooltip, option }}
/>
