<!--
  A dialog over the window: `role="dialog"`, `aria-modal`. Focus moves in
  and stays in (Tab and Shift+Tab wrap), and goes back where it was when it
  closes. Esc, the close button or a click outside closes it, unless
  `dismissible={false}`. `footer` is a snippet for its buttons.
-->
<script>
  import { tick } from "svelte";
  import Icon from "./internal/Icon.svelte";
  import { focusables, trapTab } from "./internal/focus.js";

  let {
    open = $bindable(false),
    title = undefined,
    description = undefined,
    size = "md",
    dismissible = true,
    placement = "center",
    onclose = undefined,
    class: className = "",
    children,
    footer = undefined,
  } = $props();

  const own = $props.id();
  let panel = $state();

  function close() {
    if (!open) return;
    open = false;
    onclose?.();
  }

  $effect(() => {
    if (!open) return;
    const before = document.activeElement;
    tick().then(() => (focusables(panel)[0] ?? panel)?.focus());
    return () => {
      if (before && typeof before.focus === "function" && document.contains(before)) before.focus();
    };
  });

  function onKey(event) {
    if (event.key === "Escape" && dismissible) {
      event.stopPropagation();
      close();
      return;
    }
    trapTab(event, panel);
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="elyra-overlay {placement}"
    onclick={(event) => dismissible && event.target === event.currentTarget && close()}
  >
    <div
      bind:this={panel}
      role="dialog"
      aria-modal="true"
      aria-labelledby={title ? `${own}-title` : undefined}
      aria-describedby={description ? `${own}-description` : undefined}
      tabindex="-1"
      class="elyra-dialog {size} {placement} {className}"
      onkeydown={onKey}
    >
      {#if title || dismissible}
        <header>
          {#if title}<h2 id="{own}-title">{title}</h2>{/if}
          {#if dismissible}
            <button type="button" class="close" aria-label="Close" onclick={close}><Icon name="close" /></button>
          {/if}
        </header>
      {/if}
      {#if description}<p id="{own}-description" class="description">{description}</p>{/if}
      <div class="body">{@render children?.()}</div>
      {#if footer}<footer>{@render footer()}</footer>{/if}
    </div>
  </div>
{/if}

<style>
  .elyra-overlay {
    position: fixed; inset: 0; z-index: 1000; display: flex; align-items: center; justify-content: center;
    padding: 24px; background: var(--elyra-overlay); backdrop-filter: blur(2px);
  }
  .elyra-overlay.right { justify-content: flex-end; padding: 0; }
  .elyra-overlay.left { justify-content: flex-start; padding: 0; }
  .elyra-dialog {
    display: flex; flex-direction: column; box-sizing: border-box; max-height: 100%; width: 460px; max-width: 100%;
    background: var(--elyra-raised); color: var(--elyra-text); border: 1px solid var(--elyra-border);
    border-radius: var(--elyra-radius-lg); box-shadow: var(--elyra-shadow); outline: none;
    font: var(--elyra-font-size)/1.5 var(--elyra-font);
  }
  .elyra-dialog.sm { width: 360px; }
  .elyra-dialog.lg { width: 680px; }
  .elyra-dialog.right, .elyra-dialog.left { height: 100%; border-radius: 0; border-top: 0; border-bottom: 0; width: 400px; }
  .elyra-dialog.right { border-right: 0; }
  .elyra-dialog.left { border-left: 0; }
  header { display: flex; align-items: center; gap: 8px; padding: 18px 20px 0; }
  h2 { flex: 1; margin: 0; font-size: 15px; font-weight: 600; }
  .close {
    display: inline-grid; place-items: center; width: 26px; height: 26px; margin-left: auto;
    border: 0; border-radius: var(--elyra-radius); background: transparent; color: var(--elyra-muted); cursor: pointer;
  }
  .close:hover { background: var(--elyra-inset); color: var(--elyra-text); }
  .close:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  .description { margin: 6px 20px 0; color: var(--elyra-muted); font-size: 13px; }
  .body { padding: 14px 20px 20px; overflow: auto; }
  footer { display: flex; justify-content: flex-end; gap: 8px; padding: 12px 20px; border-top: 1px solid var(--elyra-border); }
</style>
