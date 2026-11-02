<script>
  import { Table } from "@elyra/ui";

  const INVOICES = [
    { id: 1, number: "F-1042", customer: "Ada Lovelace AS", note: "Analytical engine, second instalment", total: "12 400 kr" },
    { id: 2, number: "F-1043", customer: "Grace Hopper Consulting", note: "COBOL training, two days in Arlington", total: "8 600 kr" },
    { id: 3, number: "F-1044", customer: "Linus Torvalds", note: "Kernel review", total: "3 150 kr" },
  ];
</script>

<!-- Drag a header's right edge, or focus it and press ← →; the note takes the rest. -->
<Table
  rows={INVOICES}
  label="Invoices"
  resizable
  persist="docs-invoices"
  columns={[{ key: "number", width: 90 }, { key: "customer", width: 200 }, { key: "total", align: "end", width: 110 }, "note"]}
/>
