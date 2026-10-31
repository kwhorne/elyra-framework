<!-- @elyra/ui inside a scaffolded app, for the browser smoke (harness.js). -->
<script>
  import { live, ValidationError } from "@elyra/runtime";
  import { Button, Field, Form, Input, Modal, Select, Table } from "@elyra/ui";
  // The app's own copy (`rata make:ui Field`), inside the package's Form.
  import OwnField from "./lib/ui/Field.svelte";

  let country = $state(undefined);
  let query = $state({ page: 1, per_page: 2 });
  const customers = $derived(live("customers_index", query));
  let open = $state(false);

  async function save() {
    throw new ValidationError("customers_store", { email: ["The email must be a valid email address."] }, "{}");
  }
</script>

<div id="ui-check">
  <Form onsubmit={save}>
    <OwnField name="email" label="Email"><Input /></OwnField>
    <Button type="submit">Save</Button>
  </Form>
  <Field label="Country"><Select bind:value={country} options={["Denmark", "Norway", "Sweden"]} /></Field>
  <Button onclick={() => (open = true)}>Open dialog</Button>
  <Table page={customers} bind:query columns={["name", "email"]} sortable label="Customers" />
  <Modal bind:open title="Check">
    <input aria-label="Reason" />
    {#snippet footer()}<Button onclick={() => (open = false)}>Done</Button>{/snippet}
  </Modal>
</div>
