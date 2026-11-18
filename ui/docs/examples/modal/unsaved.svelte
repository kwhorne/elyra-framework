<script>
  import { confirm } from "@elyra/runtime";
  import { Button, Field, Form, Input, Modal } from "@elyra/ui";

  let open = $state(false);
  let dirty = $state(false);
  let name = $state("");

  function unsaved() {
    return !dirty || confirm("Your changes aren't saved.", { confirmLabel: "Discard", danger: true });
  }

  async function save() {
    await new Promise((resolve) => setTimeout(resolve, 500));
    open = false;
  }
</script>

<Button onclick={() => (open = true)}>Edit customer</Button>

<Modal bind:open title="Edit customer" beforeclose={unsaved}>
  <Form bind:dirty onsubmit={save} id="edit-customer">
    <Field name="name" label="Name"><Input bind:value={name} /></Field>
  </Form>
  {#snippet footer()}
    <Button type="submit" form="edit-customer" variant="primary">Save</Button>
  {/snippet}
</Modal>
