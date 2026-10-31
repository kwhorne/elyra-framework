<script>
  import { Checkbox } from "@elyra/ui";

  let vip = $state(true);
  let newsletter = $state(false);
</script>

<Checkbox bind:checked={vip}>VIP customer</Checkbox>
<Checkbox bind:checked={newsletter}>Gets the newsletter</Checkbox>
<Checkbox disabled>Archived</Checkbox>
