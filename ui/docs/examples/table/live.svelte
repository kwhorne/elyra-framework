<script>
  import { readable } from "svelte/store";
  import { Table } from "@elyra/ui";

  // What `live.customers_index(query)` gives the table: loading, then a page.
  const customers = readable({ value: undefined, error: null, loading: true }, (set) => {
    const timer = setTimeout(() => {
      const data = [
        { id: 1, name: "Ada Lovelace", email: "ada@analytical.co" },
        { id: 2, name: "Grace Hopper", email: "grace@cobol.dev" },
      ];
      set({ value: { data, total: 2, per_page: 25, current_page: 1, last_page: 1 }, error: null, loading: false });
    }, 2000);
    return () => clearTimeout(timer);
  });
</script>

<Table page={customers} columns={["name", "email"]} label="Customers" />
