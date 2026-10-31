<script>
  import { Button, Field, Form, Input, Modal } from "@elyra/ui";

  let open = $state(false);
  let name = $state("");

  async function create() {
    await new Promise((resolve) => setTimeout(resolve, 500));
    open = false;
  }
</script>

<Button variant="primary" onclick={() => (open = true)}>New customer</Button>

<Modal bind:open title="New customer" dismissible={false}>
  <Form onsubmit={create} id="new-customer">
    <Field name="name" label="Name" required><Input bind:value={name} /></Field>
  </Form>
  {#snippet footer()}
    <Button onclick={() => (open = false)}>Cancel</Button>
    <Button type="submit" form="new-customer" variant="primary">Create</Button>
  {/snippet}
</Modal>
