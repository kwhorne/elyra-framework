<!-- @elyra/ui inside a scaffolded app, for the browser smoke (harness.js). -->
<script>
  import { live, ValidationError } from "@elyra/runtime";
  import { Button, Field, Form, Input, Modal, Select, Table } from "@elyra/ui";

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
    <Field name="email" label="Email"><Input /></Field>
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
