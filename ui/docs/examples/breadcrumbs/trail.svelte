<script>
  import { Breadcrumbs } from "@elyra/ui";

  const trail = [
    { label: "Customers", path: "/customers" },
    { label: "Acme AS", path: "/customers/7" },
    { label: "Invoices", path: "/customers/7/invoices" },
    { label: "2026-114", path: "/customers/7/invoices/114" },
    { label: "Lines" },
  ];
</script>

<div class="stack">
  <Breadcrumbs items={trail.slice(0, 3)} />
  <Breadcrumbs items={trail} max={3} />
</div>

<style>
  .stack { display: grid; gap: 12px; }
</style>
