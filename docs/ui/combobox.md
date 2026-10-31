# Combobox

Type to find one of many. It's an input that filters a list as you type,
from a list it's given or by asking a function, such as a resource's
`_index` command. Use a [Select](select.md) when the options are few and
known. Use a Combobox when there are too many to scroll, or when they
live on the server.

<!-- example: combobox/basic -->
```svelte
<script>
  import { Combobox, Field } from "@elyra/ui";

  let city = $state("Bergen");
  const CITIES = ["Bergen", "Bodø", "Drammen", "Kristiansand", "Oslo", "Stavanger", "Tromsø", "Trondheim"];
</script>

<Field label="City">
  <Combobox bind:value={city} options={CITIES} placeholder="Type a city" />
</Field>
```

## Options

`options` are strings or objects. An object's value and text are its
`value` and `label` unless `optionValue` and `optionLabel` say otherwise.
Each is a key (`optionValue="id"`) or a function
(`optionLabel={(c) => `${c.name} (${c.city})`}`). An object with
`disabled: true` is shown but can't be picked.

`bind:value` is the chosen one's value, and `onchange(value, option)` is
told when the user picks one or clears it. `option` is the object as it
came, or `null` when cleared.

## Searching on the server

Give `options` a function instead of a list. It's called with what's
typed and an `AbortSignal`, and returns the options, or a
[`Page`](../resources.md) of them. A resource's `_index` command is
already that function:

```svelte
<script>
  import { Combobox, Field } from "@elyra/ui";
  import { api } from "../bindings";

  let { invoice } = $props();
  const customers = (search) => api.customers_index({ search, per_page: 20 });
</script>

<Field name="customer_id" label="Customer">
  <Combobox
    bind:value={invoice.customer_id}
    label={invoice.customer?.name}
    options={customers}
    optionValue="id"
    optionLabel="name"
  />
</Field>
```

- **It waits for the typing to pause** (`debounce`, 200 ms) before it asks.
  Only the latest answer is shown, and the request before it gets an
  aborted signal.
- **`minChars`** holds the search until that much is typed. The list
  stays closed until then. With the default 0, opening the list asks for
  `""`, the first page.
- **`label`** is the chosen value's text, which a list from the server
  can't know until it's searched. Give it the text you have, such as the
  record's customer name. A pick sets it, and `bind:label` reads it back.
- **While it asks**, a spinner shows in the field. A failure shows in the
  list (Laravel's message, a command's `detail`).

<!-- example: combobox/search -->
```svelte
<script>
  import { Combobox, Field } from "@elyra/ui";

  // Stands in for `api.customers_index({ search })`: a page of customers.
  const CUSTOMERS = [
    { id: 1, name: "Ada Lovelace", city: "London" },
    { id: 2, name: "Grace Hopper", city: "Arlington" },
    { id: 3, name: "Kristen Nygaard", city: "Oslo" },
    { id: 4, name: "Ole-Johan Dahl", city: "Oslo" },
    { id: 5, name: "Margaret Hamilton", city: "Boston" },
  ];
  async function customers(search, { signal }) {
    await new Promise((resolve) => setTimeout(resolve, 300));
    signal.throwIfAborted();
    const data = CUSTOMERS.filter((c) => c.name.toLowerCase().includes(search.toLowerCase()));
    return { data, total: data.length, current_page: 1, last_page: 1, per_page: 20 };
  }

  let customer = $state(3);
</script>

<Field label="Customer" description={customer ? `Customer #${customer}` : "Nobody yet."}>
  <Combobox
    bind:value={customer}
    label="Kristen Nygaard"
    options={customers}
    optionValue="id"
    optionLabel="name"
    placeholder="Search customers"
  />
</Field>
```

## Leaving the field

The text is a search, not the value. When focus leaves:

- **Half-typed text** is put back to what's chosen.
- **Emptied text** clears the choice. That's the keyboard's way to clear;
  the mouse has the clear button at the end of the field, which stays out
  of the tab order and keeps focus in the input (`clearable={false}`
  hides it).

## Keyboard

| Key | |
|---|---|
| Letters | Filter, or search. The list opens and the first match is active. |
| <kbd>↓</kbd> <kbd>↑</kbd> | Open the list, then move through it, past the disabled options. |
| <kbd>Enter</kbd> | Picks the active option. While the list is open, it doesn't submit the form. |
| <kbd>Esc</kbd> | Closes the list and puts back the chosen text. Inside a Modal, it doesn't close the modal. |
| <kbd>Tab</kbd> | Closes the list without picking, puts back the chosen text, and moves on. |

A click on the field opens the list; a click outside closes it.

## Accessibility

- The input is a `role="combobox"` with `aria-autocomplete="list"`,
  `aria-expanded` and `aria-controls`. In a `Field`, it has the field's
  id, so the label names it, and it's `aria-invalid` with the field's
  error.
- The list is a `role="listbox"` labelled by the field's label, with
  `aria-busy` while it's being searched. Each option is a `role="option"`
  with `aria-selected`, and a disabled one has `aria-disabled`.
- Focus stays in the input. The option under the keys is its
  `aria-activedescendant`.
- The spinner is a `role="status"` named "Searching", and a failed search
  is a `role="alert"`.

## API

### Combobox

| Prop | Type | Default | Description |
|---|---|---|---|
| `value` (bindable) | `any` | `null` | The chosen option's value. |
| `label` (bindable) | `string` | `""` | The chosen option's text, for a value the list doesn't have yet. |
| `options` | `Array \| (query, { signal }) => Promise<Array \| Page>` | `[]` | The options, or a function that searches for them. |
| `optionValue` | `string \| (option) => any` | `"value"` | An object option's value. |
| `optionLabel` | `string \| (option) => string` | `"label"` | An object option's text. |
| `placeholder` | `string` | `"Search…"` | |
| `minChars` | `number` | `0` | With a function: how much to type before it's asked. |
| `debounce` | `number` | `200` | With a function: the pause, in ms, before it's asked. |
| `emptyText` | `string` | `"No matches"` | Shown when nothing matches. |
| `clearable` | `boolean` | `true` | A button that clears the choice. |
| `disabled` | `boolean` | `false` | |
| `onchange` | `(value, option) => void` | — | Told when the user picks one, or clears it (`null, null`). |
| `class` | `string` | `""` | Added to the combobox's outer element. |

Other attributes go to the input.
