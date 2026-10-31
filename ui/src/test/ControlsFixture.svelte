<script>
  import Field from "../Field.svelte";
  import Textarea from "../Textarea.svelte";
  import Checkbox from "../Checkbox.svelte";
  import Switch from "../Switch.svelte";
  import RadioGroup from "../RadioGroup.svelte";

  let {
    notes = $bindable(""),
    agree = $bindable(false),
    live = $bindable(false),
    plan = $bindable("monthly"),
  } = $props();
</script>

<Field label="Notes"><Textarea bind:value={notes} /></Field>
<Checkbox bind:checked={agree}>I agree</Checkbox>
<Switch bind:checked={live}>Go live</Switch>
<Field label="Plan" error={plan === "none" ? "Choose a plan." : undefined}>
  <RadioGroup bind:value={plan} options={["monthly", { value: "yearly", label: "Yearly" }, { value: "none", label: "None" }]} />
</Field>
