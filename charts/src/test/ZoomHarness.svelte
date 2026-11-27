<script>
  // A chart's window, bound (RFC 0061): the test reads `range` off the page and sets it.
  import { LineChart } from "../index.js";
  let { chart: Chart = LineChart, range = null, ...rest } = $props();
  let window = $state(range);
  export const set = (r) => (window = r);
</script>

<Chart {...rest} zoom bind:range={window} />
<output>{JSON.stringify(window)}</output>
