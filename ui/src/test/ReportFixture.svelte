<script>
  // A report of known heights (RFC 0064): `data-h` is each element's height
  // in the test, which has no layout of its own.
  import { Report } from "../index.js";
  let { rows = 10, pages = $bindable(1) } = $props();
</script>

<Report margins={{ top: 0, right: 0, bottom: 0, left: 0 }} bind:pages>
  {#snippet header({ page })}<p class="head">{page === 1 ? "Invoice 1042" : "Invoice 1042, continued"}</p>{/snippet}
  {#snippet footer({ page, pages })}<p class="foot">Page {page} of {pages}</p>{/snippet}
  <p id="first" data-h="600">Ada Lovelace</p>
  <p data-h="600">Analytical Engines Ltd</p>
  <table data-h={40 + rows * 100}>
    <thead><tr><th>Item</th></tr></thead>
    <tbody>
      {#each Array.from({ length: rows }, (_, i) => i) as i (i)}<tr data-h="100"><td>Part {i + 1}</td></tr>{/each}
    </tbody>
  </table>
</Report>
