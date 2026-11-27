<script>
  // A chart's selection, bound (RFC 0063): the test reads it off the page and sets it.
  import { LineChart } from "../index.js";
  let { chart: Chart = LineChart, selection = null, ...rest } = $props();
  let chosen = $state(selection);
  export const set = (s) => (chosen = s);
</script>

<Chart {...rest} brush bind:selection={chosen} />
<output>{JSON.stringify(chosen)}</output>
