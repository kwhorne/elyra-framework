<!--
  Someone, as a small round picture: `src` when there is one, otherwise
  their initials on a colour that's always the same for the same name. A
  picture that fails to load falls back to the initials. The name is what
  a screen reader says.
-->
<script>
  import { avatarSize, hue, initials } from "./internal/people.js";

  let { name = "", src = undefined, size = "md", class: className = "", ...rest } = $props();

  const px = $derived(avatarSize(size));

  let failed = $state(false);
  // A new picture gets a new try.
  $effect(() => {
    src;
    failed = false;
  });
</script>

<span
  class="elyra-avatar {className}"
  style:--size="{px}px"
  style:--hue={hue(name)}
  {...rest}
>
  {#if src && !failed}
    <img {src} alt={name} onerror={() => (failed = true)} />
  {:else}
    <span class="initials" role="img" aria-label={name || "Someone"}>
      <span aria-hidden="true">{initials(name)}</span>
    </span>
  {/if}
</span>

<style>
  .elyra-avatar {
    display: inline-grid; flex: none; place-items: center;
    width: var(--size); height: var(--size); border-radius: 50%; overflow: hidden;
    font: 600 calc(var(--size) * 0.4) / 1 var(--elyra-font); letter-spacing: .02em;
    vertical-align: middle; user-select: none;
  }
  img { width: 100%; height: 100%; object-fit: cover; display: block; }
  .initials {
    display: grid; place-items: center; width: 100%; height: 100%;
    background: hsl(var(--hue) 42% 44%); color: var(--elyra-on-accent);
  }
</style>
