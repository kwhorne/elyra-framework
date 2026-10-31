<!-- A panel of related content, with a title and actions (snippets). -->
<script>
  let { title = undefined, description = undefined, class: className = "", actions = undefined, footer = undefined, children, ...rest } = $props();
  const own = $props.id();
</script>

<section class="elyra-card {className}" aria-labelledby={title ? `${own}-title` : undefined} {...rest}>
  {#if title || actions}
    <div class="head">
      <div>
        {#if title}<h2 id="{own}-title">{title}</h2>{/if}
        {#if description}<p>{description}</p>{/if}
      </div>
      {#if actions}<div class="actions">{@render actions()}</div>{/if}
    </div>
  {/if}
  <div class="body">{@render children?.()}</div>
  {#if footer}<footer>{@render footer()}</footer>{/if}
</section>

<style>
  .elyra-card {
    background: var(--elyra-raised); color: var(--elyra-text); border: 1px solid var(--elyra-border);
    border-radius: var(--elyra-radius-lg); font: var(--elyra-font-size)/1.5 var(--elyra-font); min-width: 0;
  }
  .head { display: flex; align-items: flex-start; justify-content: space-between; gap: 12px; padding: 16px 18px 0; }
  h2 { margin: 0; font-size: 15px; font-weight: 600; text-wrap: balance; }
  p { margin: 2px 0 0; color: var(--elyra-muted); font-size: 13px; }
  .actions { display: flex; gap: 8px; flex: none; }
  .body { padding: 14px 18px 18px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; padding: 12px 18px; border-top: 1px solid var(--elyra-border); }
</style>
