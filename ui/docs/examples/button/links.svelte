<script>
  import { Button } from "@elyra/ui";
  import { route } from "@elyra/runtime";
</script>

<Button href="/customers/7">Open Ada Lovelace</Button>
<Button variant="ghost" href="https://elyracode.com/framework" target="_blank">The website</Button>
<span>The app is at <code>{$route.path}</code></span>
