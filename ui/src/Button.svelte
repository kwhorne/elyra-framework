<!--
  A button, or a link that looks like one (`href`). Inside a `<Form>`, a
  `type="submit"` button shows the form is busy.

  `can="customers.delete"` hides it from a user without the ability (or, with
  `canMode="disable"`, disables it); `confirm="Delete Ada?"` asks first, in
  the runtime's dialog (`confirmLabel` names its button).
-->
<script>
  import { getContext } from "svelte";
  import { can as canStore, confirm as ask, href as routeHref } from "@elyra/runtime";
  import { FORM } from "./internal/context.js";

  let {
    variant = "default",
    size = "md",
    type = "button",
    href = undefined,
    disabled = false,
    loading = false,
    can = undefined,
    canMode = "hide",
    confirm = undefined,
    confirmLabel = undefined,
    onclick = undefined,
    class: className = "",
    children,
    ...rest
  } = $props();

  const form = getContext(FORM);
  const allowed = $derived(can ? $canStore(can) : true);
  const busy = $derived(loading || (type === "submit" && !!form?.busy));
  const off = $derived(disabled || busy || !allowed);
  // An app path (`/customers/7`) goes through the router.
  const target = $derived(href?.startsWith("/") ? routeHref(href) : href);

  async function click(event) {
    if (off) {
      event.preventDefault();
      return;
    }
    if (confirm) {
      event.preventDefault();
      // Gone once the event has been dispatched: kept for after the question.
      const button = event.currentTarget;
      const yes = await ask(confirm, { danger: variant === "danger", confirmLabel });
      if (!yes) return;
      if (target) {
        location.href = target;
        return;
      }
      if (type === "submit") {
        button?.form?.requestSubmit();
        return;
      }
    }
    await onclick?.(event);
  }
</script>

{#if allowed || canMode === "disable"}
  {#if target && !off}
    <a href={target} class="elyra-button {variant} {size} {className}" onclick={click} {...rest}>
      {@render children?.()}
    </a>
  {:else}
    <button
      {type}
      class="elyra-button {variant} {size} {className}"
      disabled={off}
      aria-busy={busy || undefined}
      onclick={click}
      {...rest}
    >
      {#if busy}<span class="spinner" aria-hidden="true"></span>{/if}
      {@render children?.()}
    </button>
  {/if}
{/if}

<style>
  .elyra-button {
    display: inline-flex; align-items: center; justify-content: center; gap: 6px;
    height: 32px; padding: 0 12px; box-sizing: border-box;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius);
    background: var(--elyra-inset); color: var(--elyra-text);
    font: 500 13px/1 var(--elyra-font); text-decoration: none; white-space: nowrap;
    cursor: pointer; user-select: none;
    transition: background-color .12s, border-color .12s, color .12s;
  }
  .elyra-button:hover:not(:disabled) { border-color: var(--elyra-accent); }
  .elyra-button:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  .elyra-button:disabled { opacity: .5; cursor: default; }
  .elyra-button.sm { height: 26px; padding: 0 9px; font-size: 12px; }

  .elyra-button.primary { background: var(--elyra-accent); border-color: var(--elyra-accent); color: var(--elyra-on-accent); }
  .elyra-button.primary:hover:not(:disabled) { background: color-mix(in srgb, var(--elyra-accent) 88%, var(--elyra-text)); }
  .elyra-button.ghost { background: transparent; border-color: transparent; color: var(--elyra-muted); }
  .elyra-button.ghost:hover:not(:disabled) { background: var(--elyra-inset); border-color: transparent; color: var(--elyra-text); }
  .elyra-button.danger { background: var(--elyra-danger); border-color: var(--elyra-danger); color: var(--elyra-on-accent); }
  .elyra-button.danger:hover:not(:disabled) { background: color-mix(in srgb, var(--elyra-danger) 88%, var(--elyra-text)); }

  .spinner {
    width: 12px; height: 12px; border-radius: 50%;
    border: 2px solid currentColor; border-right-color: transparent;
    animation: elyra-spin .7s linear infinite;
  }
  @keyframes elyra-spin { to { transform: rotate(360deg); } }
</style>
