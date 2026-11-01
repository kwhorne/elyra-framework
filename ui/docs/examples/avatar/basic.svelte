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
