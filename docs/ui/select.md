# Select

Choose one of a list. It's a button that opens the list, with a search box
when the list is long. It's not the browser's `<select>`, so it looks the
same on every platform and can be searched, and it's still used entirely
from the keyboard.

<!-- example: select/basic -->
```svelte
<script>
  import { Field, Select } from "@elyra/ui";

  let country = $state("Norway");
</script>

<Field label="Country">
  <Select bind:value={country} options={["Denmark", "Finland", "Iceland", "Norway", "Sweden"]} />
</Field>
```

## Options

`options` are strings, or `{ value, label, disabled }` when the value isn't
what's shown: an id, say. `bind:value` is the chosen one's `value`, and
`placeholder` shows while there's none.

<!-- example: select/options -->
```svelte
<script>
  import { Field, Select } from "@elyra/ui";

  let team = $state(undefined);
  const TEAMS = [
    { value: 1, label: "Sales" },
    { value: 2, label: "Support" },
    { value: 3, label: "Operations", disabled: true },
  ];
</script>

<Field label="Team" description={team ? `Team #${team}` : "Nobody's yet."}>
  <Select bind:value={team} options={TEAMS} placeholder="No team" />
</Field>
```

`onchange(value)` is told when the user picks one.

## Search

From 8 options, the list has a search box. Typing filters it, and
<kbd>Enter</kbd> picks the first match. Set `searchable` to have the box, or
not, whatever the length. Without one, typing jumps to the first option that
starts with what was typed.

<!-- example: select/search -->
```svelte
<script>
  import { Field, Select } from "@elyra/ui";

  let currency = $state("NOK");
  const CURRENCIES = ["CHF", "DKK", "EUR", "GBP", "ISK", "JPY", "NOK", "PLN", "SEK", "USD"];
</script>

<!-- From 8 options there's a search box; typing filters the list. -->
<Field label="Currency">
  <Select bind:value={currency} options={CURRENCIES} />
</Field>
```

## Keyboard

| Key | |
|---|---|
| <kbd>↓</kbd> <kbd>↑</kbd>, <kbd>Enter</kbd>, <kbd>Space</kbd> | On the button: opens the list, at the chosen option. |
| <kbd>↓</kbd> <kbd>↑</kbd> | Moves through the options, past the disabled ones. |
| <kbd>Home</kbd> <kbd>End</kbd> | The first option, the last option. |
| <kbd>Enter</kbd> | Picks the option. |
| <kbd>Esc</kbd> | Closes the list; focus goes back to the button. |
| <kbd>Tab</kbd> | Closes the list and moves on. |
| Letters | With a search box, filters. Without one, jumps to the option they start. |

A click outside closes it.

## Accessibility

- The button is a `role="combobox"` with `aria-haspopup="listbox"`,
  `aria-expanded` and `aria-controls`. In a `Field`, it has the field's id,
  so the label names it.
- The list is a `role="listbox"`, labelled by the field's label. Each option
  is a `role="option"` with `aria-selected`, and a disabled one has
  `aria-disabled`.
- The option under the keys is the list's (or the search box's)
  `aria-activedescendant`. Focus stays in one place while the keys move
  through the options.

## API

### Select

| Prop | Type | Default | Description |
|---|---|---|---|
| `value` (bindable) | `any` | — | The chosen option's value. |
| `options` | `Array<string \| { value, label, disabled? }>` | `[]` | |
| `placeholder` | `string` | `"Choose…"` | Shown while nothing is chosen. |
| `searchable` | `boolean` | — | A search box. Without it set, there is one from 8 options. |
| `disabled` | `boolean` | `false` | |
| `onchange` | `(value) => void` | — | Told when the user picks one. |
| `class` | `string` | `""` | Added to the select's outer element. |

Other attributes go to the button.
