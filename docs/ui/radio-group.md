# RadioGroup

One of a few choices, all in sight. These are native radios, drawn to match
the rest. For a longer list, use a [Select](select.md).

<!-- example: radio-group/basic -->
```svelte
<script>
  import { Field, RadioGroup } from "@elyra/ui";

  let billing = $state("yearly");
</script>

<Field label="Billing">
  <RadioGroup
    bind:value={billing}
    inline
    options={[
      { value: "monthly", label: "Monthly" },
      { value: "yearly", label: "Yearly" },
      { value: "lifetime", label: "Lifetime", disabled: true },
    ]}
  />
</Field>
```

`options` are strings, or `{ value, label, disabled }`. `bind:value` is the
chosen one's value. `inline` puts them on one line.

## Outside a field

In a [Field](form.md), the field's label names the group. Outside one, give
it a `label`.

<!-- example: radio-group/stacked -->
```svelte
<script>
  import { RadioGroup } from "@elyra/ui";

  let delivery = $state("Email");
</script>

<RadioGroup label="Send invoices by" bind:value={delivery} options={["Email", "EHF", "Post"]} />
```

## Keyboard

| Key | |
|---|---|
| <kbd>Tab</kbd> | Into the group, on the chosen option, and out again. |
| <kbd>↓</kbd> <kbd>→</kbd>, <kbd>↑</kbd> <kbd>←</kbd> | Choose the next or the previous option. |

## Accessibility

- A `role="radiogroup"`, named by `label` or by the field's label. In a
  `Field`, it has `aria-describedby` and `aria-invalid`.
- The radios share a name: the field's, or one of the group's own. The
  browser moves between them.

## API

### RadioGroup

| Prop | Type | Default | Description |
|---|---|---|---|
| `value` (bindable) | `any` | — | The chosen option's value. |
| `options` | `Array<string \| { value, label, disabled? }>` | `[]` | |
| `label` | `string` | — | The group's name, outside a `Field`. |
| `inline` | `boolean` | `false` | On one line. |
| `class` | `string` | `""` | |

Other attributes go to the group's `<div>`.
