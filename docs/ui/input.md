# Input and Textarea

A text input and a text area, drawn on the tokens. Inside a
[Field](form.md), they take the field's id, its name, and its state.

<!-- example: input/basic -->
```svelte
<script>
  import { Field, Input } from "@elyra/ui";

  let name = $state("");
</script>

<Field label="Name">
  <Input bind:value={name} placeholder="Ada Lovelace" autocomplete="name" />
</Field>
```

## Types

`type` is the input's: `text`, `email`, `number`, `search`, `password`,
`url`, `tel`, `date`. Other attributes go to the `<input>`: `placeholder`,
`min`, `step`, `autocomplete`, `disabled`, `readonly`.

<!-- example: input/types -->
```svelte
<script>
  import { Field, Input } from "@elyra/ui";
</script>

<Field label="Email"><Input type="email" placeholder="ada@example.com" /></Field>
<Field label="Credit limit"><Input type="number" min="0" step="100" value="5000" /></Field>
<Field label="Find"><Input type="search" placeholder="Search customers…" /></Field>
<Field label="Disabled"><Input value="Can't change this" disabled /></Field>
```

On a `number` input, `bind:value` is a number, or `null` while the input is
empty.

## Textarea

`rows` is its height, in lines. It grows no further on its own. The user can
drag it taller.

<!-- example: input/textarea -->
```svelte
<script>
  import { Field, Textarea } from "@elyra/ui";

  let notes = $state("Prefers invoices by email.");
</script>

<Field label="Notes" description="Everyone who can see the customer sees these.">
  <Textarea bind:value={notes} rows={3} />
</Field>
```

## Accessibility

- Native controls: the keys, selection and autofill are the browser's.
- In a `Field`, the label points at the control. The help or the error
  under it is its `aria-describedby`. While there's an error it has
  `aria-invalid`, and a required field's control is `required`.
- Outside a `Field`, give it a name: `aria-label="Search"`.

## API

### Input

| Prop | Type | Default | Description |
|---|---|---|---|
| `value` (bindable) | `string \| number \| null` | `""` | A number on a `number` input. |
| `type` | `string` | `"text"` | The input's type. |
| `class` | `string` | `""` | |

Other attributes go to the `<input>`.

### Textarea

| Prop | Type | Default | Description |
|---|---|---|---|
| `value` (bindable) | `string` | `""` | |
| `rows` | `number` | `4` | Its height, in lines. |
| `class` | `string` | `""` | |

Other attributes go to the `<textarea>`.
