<!-- Nothing here yet: what would be, and the action that starts it (a snippet). -->
<script>
  let { title = "Nothing here yet", description = undefined, class: className = "", action = undefined } = $props();
</script>

<div class="elyra-empty {className}">
  <p class="title">{title}</p>
  {#if description}<p class="description">{description}</p>{/if}
  {#if action}<div class="action">{@render action()}</div>{/if}
</div>

<style>
  .elyra-empty { display: grid; justify-items: center; gap: 4px; padding: 32px 16px; text-align: center; font: 13px/1.5 var(--elyra-font); }
  .title { margin: 0; color: var(--elyra-text); font-size: 14px; font-weight: 600; }
  .description { margin: 0; max-width: 46ch; color: var(--elyra-muted); }
  .action { margin-top: 10px; }
</style>
