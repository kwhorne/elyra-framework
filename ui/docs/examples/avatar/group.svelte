<script>
  import { AvatarGroup } from "@elyra/ui";

  const viewing = ["Ada Lovelace", "Grace Hopper", "Linus Torvalds", "Margaret Hamilton", "Ken Thompson", "Barbara Liskov"].map(
    (name) => ({ name }),
  );
</script>

<!-- Six people, four places: three, and a count of the rest. -->
<AvatarGroup people={viewing} max={4} label="Also viewing" />
