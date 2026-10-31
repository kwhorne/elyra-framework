<script>
  import { Skeleton } from "@elyra/ui";
</script>

<div class="profile">
  <Skeleton width="44px" height="44px" />
  <Skeleton lines={3} />
</div>

<style>
  .profile { display: grid; grid-template-columns: auto 1fr; gap: 14px; width: 100%; max-width: 420px; }
</style>
