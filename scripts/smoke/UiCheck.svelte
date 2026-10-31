<!-- @elyra/ui inside a scaffolded app, for the browser smoke (harness.js). -->
<script>
  import { ValidationError } from "@elyra/runtime";
  import { Button, Field, Form, Input, Modal, Select } from "@elyra/ui";

  let country = $state(undefined);
  let open = $state(false);

  async function save() {
    throw new ValidationError("customers_store", { email: ["The email must be a valid email address."] }, "{}");
  }
</script>

<div id="ui-check">
  <Form onsubmit={save}>
    <Field name="email" label="Email"><Input /></Field>
    <Button type="submit">Save</Button>
  </Form>
  <Field label="Country"><Select bind:value={country} options={["Denmark", "Norway", "Sweden"]} /></Field>
  <Button onclick={() => (open = true)}>Open dialog</Button>
  <Modal bind:open title="Check">
    <input aria-label="Reason" />
    {#snippet footer()}<Button onclick={() => (open = false)}>Done</Button>{/snippet}
  </Modal>
</div>
