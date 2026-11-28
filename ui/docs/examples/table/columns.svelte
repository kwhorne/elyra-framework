<script>
  import { Table } from "@elyra/ui";

  const CUSTOMERS = [
    { id: 1, name: "Ada Lovelace", email: "ada@analytical.co", phone: "+47 22 00 00 01", org_number: "912 345 678", balance: 12400 },
    { id: 2, name: "Grace Hopper", email: "grace@cobol.dev", phone: "+47 22 00 00 02", org_number: "998 877 665", balance: 860 },
    { id: 3, name: "Kristen Nygaard", email: "kristen@simula.no", phone: "+47 22 00 00 03", org_number: "987 654 321", balance: 3150 },
  ];
</script>

<!-- Columns: show the org. number, hide the phone, Alt+↑/↓ to move one; or drag a header. -->
<Table
  rows={CUSTOMERS}
  label="Customers"
  choosable
  resizable
  persist="docs-customers"
  columns={[
    { key: "name", required: true },
    "email",
    "phone",
    { key: "org_number", label: "Org. no.", hidden: true },
    { key: "balance", align: "end", hidden: true, total: "sum" },
  ]}
/>
