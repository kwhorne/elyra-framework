<script>
  import { Field, Select } from "@elyra/ui";

  let currency = $state("NOK");
  const CURRENCIES = ["CHF", "DKK", "EUR", "GBP", "ISK", "JPY", "NOK", "PLN", "SEK", "USD"];
</script>

<!-- From 8 options there's a search box; typing filters the list. -->
<Field label="Currency">
  <Select bind:value={currency} options={CURRENCIES} />
</Field>
