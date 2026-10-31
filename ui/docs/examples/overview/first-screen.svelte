<script>
  import { ValidationError } from "@elyra/runtime";
  import { Button, Field, Form, Input, Select } from "@elyra/ui";

  let customer = $state({ name: "Ada Lovelace", email: "ada@", country: "Norway" });
  let saved = $state(false);

  // In an app this is `api.customers_store(customer)`: Laravel's 422 (or a
  // command's ValidationErrors) is thrown the same way.
  async function save() {
    saved = false;
    await new Promise((resolve) => setTimeout(resolve, 600));
    if (!customer.email.includes(".")) {
      throw new ValidationError("customers_store", { email: ["The email must be a valid email address."] }, "{}");
    }
    saved = true;
  }
</script>

<Form onsubmit={save}>
  <Field name="name" label="Name" required>
    <Input bind:value={customer.name} />
  </Field>
  <Field name="email" label="Email" required description="Invoices go here.">
    <Input type="email" bind:value={customer.email} />
  </Field>
  <Field name="country" label="Country">
    <Select bind:value={customer.country} options={["Denmark", "Finland", "Norway", "Sweden"]} />
  </Field>
  <Button type="submit" variant="primary">{saved ? "Saved" : "Save customer"}</Button>
</Form>
