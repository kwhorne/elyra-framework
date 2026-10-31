<script>
  import { Pagination } from "@elyra/ui";

  let page = $state(4);
</script>

<Pagination bind:page lastPage={12} label="Invoices" />
<span>Page {page} of 12</span>
