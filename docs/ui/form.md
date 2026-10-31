# Form and Field

A form that knows Laravel's answers. `onsubmit` runs the save. When it
throws a validation failure, each message lands under the field it belongs
to. While it runs, the form is busy and its submit button says so.

<!-- example: form/validation -->
```svelte
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
```

The save throws what Laravel answers: one message for the email, which the
form shows, and one for `team_id`, which it doesn't. The first goes under
its field. The second goes to the top, so nothing is lost.

## In an app

`onsubmit` is the save: a command, or the Laravel API through the bindings.
Return its promise, and the form waits for it.

```svelte
<script>
  import { navigate } from "@elyra/runtime";
  import { api } from "./bindings";
  import { Button, Field, Form, Input } from "@elyra/ui";

  let customer = $state({ name: "", email: "" });

  async function save() {
    const created = await api.customers_store(customer);
    navigate(`/customers/${created.id}`);
  }
</script>

<Form onsubmit={save}>
  <Field name="name" label="Name" required><Input bind:value={customer.name} /></Field>
  <Field name="email" label="Email"><Input type="email" bind:value={customer.email} /></Field>
  <Button type="submit" variant="primary">Save</Button>
</Form>
```

What it catches:

- **A validation failure:** a `ValidationError` from a command with
  `ValidationErrors`, or Laravel's `422` through the backend. It's the same
  `{ field: [messages] }` either way (`validationErrors()` in
  `@elyra/runtime` reads it). A message goes under the `Field` with that
  `name`. Messages for names the form doesn't show go to the top.
- **Anything else:** the error's message, at the top (`role="alert"`).

Each submit clears the last one's messages first. A submit while the form is
busy is ignored.

## Field

A label, a control, and its help or its error. The control inside, whether
an `Input`, `Textarea`, `Select`, `Checkbox`, `Switch` or `RadioGroup`, gets
the field's id, so the label points at it. It also gets `aria-invalid`, and
`aria-describedby` for the message under it.

<!-- example: form/field -->
```svelte
<script>
  import { Field, Input } from "@elyra/ui";
</script>

<Field label="Company" description="As it appears on invoices.">
  <Input placeholder="Analytical Engines Ltd" />
</Field>
<Field label="VAT number" required error="That isn't a Norwegian VAT number.">
  <Input value="NO 123" />
</Field>
```

- `name` takes the field's message from the form's errors.
- `error` sets a message directly, inside a form or not.
- `description` is help under the control. An error replaces it while
  there is one.
- `required` marks the label, and makes an `Input` or a `Textarea`
  `required`.

## Your own checks

`bind:errors` reads the form's messages, or sets them. Check before anything
is sent, and the messages land the same way:

<!-- example: form/errors -->
```svelte
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
```

## Accessibility

- The form is `novalidate`: the messages are the app's, under the fields,
  not the browser's bubbles.
- While it saves, the form has `aria-busy`, and so does its submit button.
- The label is a `<label for>` the control's id. The message under the
  control is its `aria-describedby`, and an invalid control has
  `aria-invalid`.
- The message at the top is a `role="alert"`, so it's read when it appears.

## API

### Form

| Prop | Type | Default | Description |
|---|---|---|---|
| `onsubmit` | `(event) => void \| Promise<void>` | — | The save. What it throws is shown. |
| `errors` (bindable) | `Record<string, string[]>` | `{}` | The messages, by field name. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The fields and the buttons. |

Other attributes go to the `<form>`, such as an `id` that a submit button
outside it names with `form`.

### Field

| Prop | Type | Default | Description |
|---|---|---|---|
| `name` | `string` | — | The field's name in the form's errors. |
| `label` | `string` | — | |
| `description` | `string` | — | Help under the control. |
| `error` | `string` | — | A message of its own, before the form's. |
| `required` | `boolean` | `false` | Marks the label, and makes an `Input` or `Textarea` `required`. |
| `class` | `string` | `""` | |
| `children` | `Snippet` | — | The control. |
