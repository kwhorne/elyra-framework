<script>
  import { Field, Input } from "@elyra/ui";
</script>

<Field label="Company" description="As it appears on invoices.">
  <Input placeholder="Analytical Engines Ltd" />
</Field>
<Field label="VAT number" required error="That isn't a Norwegian VAT number.">
  <Input value="NO 123" />
</Field>
