<script>
  import { ValidationError } from "@elyra/runtime";
  import { Button, Field, Form, Input } from "@elyra/ui";

  let email = $state("ada@analytical");
  let team = $state("Sales");

  // What Laravel answers for this form: one message for a field it shows,
  // one for a field it doesn't.
  async function save() {
    await new Promise((resolve) => setTimeout(resolve, 600));
    throw new ValidationError(
      "customers_store",
      {
        email: ["The email must be a valid email address."],
        team_id: ["The Sales team has no seats left."],
      },
      "{}",
    );
  }
</script>

<Form onsubmit={save}>
  <Field name="email" label="Email" required>
    <Input type="email" bind:value={email} />
  </Field>
  <Field label="Team">
    <Input bind:value={team} />
  </Field>
  <Button type="submit" variant="primary">Save</Button>
</Form>
