<script>
  import { Field, RadioGroup } from "@elyra/ui";

  let billing = $state("yearly");
</script>

<Field label="Billing">
  <RadioGroup
    bind:value={billing}
    inline
    options={[
      { value: "monthly", label: "Monthly" },
      { value: "yearly", label: "Yearly" },
      { value: "lifetime", label: "Lifetime", disabled: true },
    ]}
  />
</Field>
