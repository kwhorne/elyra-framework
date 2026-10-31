<script>
  import { Combobox, Field } from "@elyra/ui";

  // Stands in for `api.customers_index({ search })`: a page of customers.
  const CUSTOMERS = [
    { id: 1, name: "Ada Lovelace", city: "London" },
    { id: 2, name: "Grace Hopper", city: "Arlington" },
    { id: 3, name: "Kristen Nygaard", city: "Oslo" },
    { id: 4, name: "Ole-Johan Dahl", city: "Oslo" },
    { id: 5, name: "Margaret Hamilton", city: "Boston" },
  ];
  async function customers(search, { signal }) {
    await new Promise((resolve) => setTimeout(resolve, 300));
    signal.throwIfAborted();
    const data = CUSTOMERS.filter((c) => c.name.toLowerCase().includes(search.toLowerCase()));
    return { data, total: data.length, current_page: 1, last_page: 1, per_page: 20 };
  }

  let customer = $state(3);
</script>

<Field label="Customer" description={customer ? `Customer #${customer}` : "Nobody yet."}>
  <Combobox
    bind:value={customer}
    label="Kristen Nygaard"
    options={customers}
    optionValue="id"
    optionLabel="name"
    placeholder="Search customers"
  />
</Field>
