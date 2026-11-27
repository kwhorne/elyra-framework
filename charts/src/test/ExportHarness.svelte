<script>
  import { LineChart } from "../index.js";
  let { series, categories, renderer = "svg" } = $props();
  export const chart = { save: (f) => line.save(f), copy: () => line.copy(), csv: () => line.csv() };
  let line;
</script>

<LineChart bind:this={line} {series} {categories} {renderer} />
