<!-- One example: running, and its code under it. -->
<script>
  import { highlight } from "./highlight.js";

  let { name, component: Component, code } = $props();

  let copied = $state(false);
  let source = $state();
  let timer;

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      copied = true;
      clearTimeout(timer);
      timer = setTimeout(() => (copied = false), 1600);
    } catch {
      // No clipboard here: select the code, so ⌘C does it.
      const range = document.createRange();
      range.selectNodeContents(source);
      getSelection()?.removeAllRanges();
      getSelection()?.addRange(range);
    }
  }
</script>

<figure class="example">
  <div class="preview">
    {#if Component}<Component />{:else}<p class="missing">No example called {name}.</p>{/if}
  </div>
  <div class="code">
    <div class="bar">
      <span class="file">{name}.svelte</span>
      <button type="button" class="copy" onclick={copy} aria-live="polite">{copied ? "Copied" : "Copy"}</button>
    </div>
    <pre bind:this={source}><code>{@html highlight(code.trimEnd())}</code></pre>
  </div>
</figure>

<style>
  .example {
    margin: 20px 0 28px;
    border: 1px solid var(--elyra-border);
    border-radius: var(--elyra-radius-lg);
    background: var(--elyra-surface);
  }
  .preview {
    display: flex; flex-wrap: wrap; align-items: flex-start; gap: 12px;
    padding: 28px 24px;
    min-height: 72px;
    border-radius: var(--elyra-radius-lg) var(--elyra-radius-lg) 0 0;
    background:
      radial-gradient(circle at 1px 1px, color-mix(in srgb, var(--elyra-border) 70%, transparent) 1px, transparent 0) 0 0 / 16px 16px,
      var(--elyra-bg);
  }
  .preview > :global(*) { min-width: 0; max-width: 100%; }
  /* What fills a page in an app fills the preview; a form, at a form's width. */
  .preview > :global(:is(.elyra-form, .elyra-table, .elyra-card, .elyra-tabs, .elyra-app-shell, .elyra-callout,
      .elyra-empty, .elyra-field, .elyra-skeleton, .elyra-separator.horizontal, .elyra-toolbar, .elyra-status-bar, .wide)) {
    flex: 1 1 100%;
  }
  .preview > :global(:is(.elyra-form, .elyra-field)) { max-width: min(440px, 100%); }
  .missing { color: var(--elyra-danger); margin: 0; }
  .code { border-top: 1px solid var(--elyra-border); min-width: 0; }
  .bar {
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    padding: 6px 8px 6px 14px;
    border-bottom: 1px solid color-mix(in srgb, var(--elyra-border) 60%, transparent);
  }
  .file { font: 12px var(--elyra-font-mono); color: var(--elyra-muted); }
  .copy {
    font: inherit; font-size: 12px; color: var(--elyra-muted);
    background: none; border: 1px solid transparent; border-radius: 6px;
    padding: 3px 8px; cursor: pointer;
  }
  .copy:hover { color: var(--elyra-text); border-color: var(--elyra-border); }
  .copy:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  pre {
    margin: 0; padding: 14px 16px 16px;
    overflow-x: auto; max-height: 420px;
    font: 12.5px/1.65 var(--elyra-font-mono);
    color: var(--elyra-text);
    tab-size: 2;
  }
</style>
