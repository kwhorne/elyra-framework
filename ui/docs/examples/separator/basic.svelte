<script>
  import { Button, Separator } from "@elyra/ui";
</script>

<div class="row">
  <Button size="sm" variant="ghost">Bold</Button>
  <Button size="sm" variant="ghost">Italic</Button>
  <Separator orientation="vertical" />
  <Button size="sm" variant="ghost">Link</Button>
</div>
<Separator />
<p>Below the line.</p>

<style>
  .row { display: flex; align-items: center; gap: 4px; height: 28px; }
</style>
