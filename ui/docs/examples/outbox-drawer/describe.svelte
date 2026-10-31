<script>
  import { Button, OutboxDrawer } from "@elyra/ui";

  let open = $state(false);

  // "PUT /api/customers/42" as the user would say it.
  const VERBS = { POST: "New", PUT: "Changed", PATCH: "Changed", DELETE: "Deleted" };
  function describe(write) {
    const [, , kind, id] = write.path.split("/");
    const thing = kind.replace(/s$/, "");
    return `${VERBS[write.method]} ${thing}${id ? ` ${id}` : ""}`;
  }
</script>

<Button onclick={() => (open = true)}>Show changes</Button>
<OutboxDrawer bind:open {describe} title="Not sent yet" />
