# FileField

A file to send with the form. It's a button that opens the native file
dialog, with the chosen file's name beside it. The file is a
[`ChosenFile`](../backend.md#files), which is what a command's file
argument takes. `make:resource` uses it for a `file` field.

<!-- example: file-field/basic -->
```svelte
<script>
  import { Field, FileField } from "@elyra/ui";

  let receipt = $state(null);
</script>

<Field label="Receipt" description="A PDF or a photo of it.">
  <FileField
    bind:file={receipt}
    filters={[{ name: "Receipts", extensions: ["pdf", "jpg", "png"] }]}
    acceptsDrop
  />
</Field>
```

- **`bind:file`** is the chosen file, or `null`. Cancelling the dialog keeps
  what was there.
- **`filters`** go to the dialog: `{ name, extensions }`, as in
  [`dialog.open`](../system.md).
- **`acceptsDrop`** also takes a file dropped on the window while the field
  is shown. A drop is the window's, so give it to one field per form. A
  dropped folder is ignored.
- **`onchange(file)`** is told when the file changes: chosen, dropped or
  taken back (`null`).

The app needs elyra's `system` feature for the dialog. Without it, the
field shows the error the dialog gives.

## The record's file

Editing a record that has a file, `current` is its name. A new file
replaces it, and "Keep the current one" takes the new one back. Without
`current`, that button says "Remove".

<!-- example: file-field/current -->
```svelte
<script>
  import { Field, FileField } from "@elyra/ui";

  // Editing: the record has a contract already. A new one replaces it on save.
  const invoice = { id: 2041, contract: "contract-2026.pdf" };
  let contract = $state(null);
</script>

<Field label="Contract" description={contract ? "Replaces the current one on save." : undefined}>
  <FileField bind:file={contract} current={invoice.contract} />
</Field>
```

## In a form

The file goes beside the input, as the command takes it. Laravel's message
for the file shows under its `Field` like any other:

```svelte
<script>
  import { Button, Field, FileField, Form, Input } from "@elyra/ui";
  import { api } from "../bindings";

  let form = $state({ number: "" });
  let pdf = $state(null);

  const save = () => api.invoices_store(form, pdf);
</script>

<Form onsubmit={save}>
  <Field name="number" label="Number"><Input bind:value={form.number} /></Field>
  <Field name="pdf" label="PDF"><FileField bind:file={pdf} acceptsDrop /></Field>
  <Button type="submit" variant="primary">Save</Button>
</Form>
```

## Accessibility

- The button has the field's id, so the label names it. It's described by
  the file's name, then the field's help or error, and it's `aria-invalid`
  with the error.
- While the dialog is open, the button is busy (`aria-busy`) and can't be
  pressed again.
- A failed dialog is a `role="alert"`.

## API

### FileField

| Prop | Type | Default | Description |
|---|---|---|---|
| `file` (bindable) | `ChosenFile \| null` | `null` | The chosen file. |
| `current` | `string \| null` | `null` | The name of the file the record has. |
| `filters` | `Array<{ name, extensions }>` | `[]` | The dialog's filters. |
| `acceptsDrop` | `boolean` | `false` | Take a file dropped on the window. |
| `disabled` | `boolean` | `false` | |
| `onchange` | `(file) => void` | — | Told when the file changes. |
| `class` | `string` | `""` | Added to the field's outer element. |

Other attributes go to the button.
