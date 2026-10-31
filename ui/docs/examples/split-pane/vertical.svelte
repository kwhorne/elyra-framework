<script>
  import { SplitPane } from "@elyra/ui";

  let note = $state("Dear Ada,\n\nThe invoice for October is attached.");
</script>

<div class="window">
  <SplitPane direction="vertical" size={110} min={60} minSecond={60} label="Resize the preview">
    {#snippet first()}
      <textarea bind:value={note} aria-label="The note"></textarea>
    {/snippet}
    {#snippet second()}
      <div class="preview">{note}</div>
    {/snippet}
  </SplitPane>
</div>

<style>
  .window { width: 100%; height: 240px; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); overflow: hidden; }
  textarea {
    width: 100%; height: 100%; box-sizing: border-box; padding: 12px; border: 0; resize: none;
    background: var(--elyra-inset); color: var(--elyra-text); font: 13px var(--elyra-font-mono);
  }
  .preview { padding: 12px; white-space: pre-wrap; color: var(--elyra-text); }
</style>
