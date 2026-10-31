<script>
  import { Button, Field, Form, Input } from "@elyra/ui";

  let errors = $state({});
  let code = $state("");

  // Checked here, before anything is sent.
  function check() {
    errors = /^[A-Z]{3}$/.test(code) ? {} : { code: ["Three capital letters, like NOK."] };
  }
</script>

<Form bind:errors onsubmit={check}>
  <Field name="code" label="Currency">
    <Input bind:value={code} placeholder="NOK" />
  </Field>
  <Button type="submit">Check</Button>
</Form>
