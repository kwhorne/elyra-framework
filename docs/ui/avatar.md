# Avatar

Someone, as a small round picture. Without one, or when it fails to load,
their initials on a colour that's always the same for the same name, so
a person is recognisable across screens.

<!-- example: avatar/basic -->
```svelte
<script>
  import { Avatar } from "@elyra/ui";
</script>

<!-- Without a picture, the initials on the name's own colour. -->
<div class="row">
  <Avatar name="Ada Lovelace" size="sm" />
  <Avatar name="Ada Lovelace" />
  <Avatar name="Ada Lovelace" size="lg" />
  <Avatar name="Grace Hopper" size="lg" />
  <Avatar name="Øyvind Berg" size="lg" />
  <Avatar name="Linus" size="lg" src="/a-picture-that-is-not-there.png" />
</div>

<style>
  .row { display: flex; align-items: center; gap: 12px; }
</style>
```

- **Initials** are the first letters of the first and last names: "Ada
  Lovelace" is AL, "Øyvind" is Ø.
- **The colour** comes from the name, so it's the same in every window
  and for every user. It's a hue on the theme's accent text colour.
- **Sizes** are `sm` (20 px), `md` (28 px) and `lg` (40 px), or a number
  of pixels.

## A group

An AvatarGroup shows a few people overlapping: the first `max`, and a
count of the rest in the last place, with their names as its label and
tooltip.

<!-- example: avatar/group -->
```svelte
<script>
  import { AvatarGroup } from "@elyra/ui";

  const viewing = ["Ada Lovelace", "Grace Hopper", "Linus Torvalds", "Margaret Hamilton", "Ken Thompson", "Barbara Liskov"].map(
    (name) => ({ name }),
  );
</script>

<!-- Six people, four places: three, and a count of the rest. -->
<AvatarGroup people={viewing} max={4} label="Also viewing" />
```

## Accessibility

- An Avatar is an image named by the person: the `<img>`'s `alt`, or
  `role="img"` with the name as its label for the initials. The initials
  themselves are hidden from a screen reader.
- An AvatarGroup is a `role="group"` named by `label`. The count says how
  many more and who: "3 more: Margaret Hamilton, Ken Thompson, Barbara
  Liskov".

## API

### Avatar

| Prop | Type | Default | Description |
|---|---|---|---|
| `name` | `string` | `""` | Who it is: the label, the initials and the colour. |
| `src` | `string` | — | A picture. The initials without it, or when it fails. |
| `size` | `"sm" \| "md" \| "lg" \| number` | `"md"` | 20, 28 or 40 pixels, or a number. |
| `class` | `string` | `""` | |

Other attributes go to the outer `<span>`.

### AvatarGroup

| Prop | Type | Default | Description |
|---|---|---|---|
| `people` | `{ name, src? }[]` | `[]` | Who to show, in order. |
| `max` | `number` | `4` | Places, the count of the rest included. |
| `size` | `"sm" \| "md" \| "lg" \| number` | `"md"` | Each avatar's size. |
| `label` | `string` | `"People"` | The group's name for a screen reader. |
| `class` | `string` | `""` | |

Other attributes go to the outer `<div>`.
