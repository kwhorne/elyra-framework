<!--
  Who else has this open (RFC 0015): the other members of a Laravel
  presence channel, as an AvatarGroup. It joins `channel` while it's on
  screen and leaves when it goes. Nothing shows while nobody else is
  there, offline, or in an app without presence (or an older runtime).
-->
<script>
  // Read off the module: a runtime from before 0.26 has no presence.
  import * as runtime from "@elyra/runtime";
  import AvatarGroup from "./AvatarGroup.svelte";

  let {
    channel,
    self = false,
    max = 4,
    size = "sm",
    label = "Also here",
    class: className = "",
    ...rest
  } = $props();

  let here = $state(null);
  $effect(() => {
    const store = runtime.presence?.(channel);
    if (!store) return;
    return store.subscribe((state) => (here = state));
  });

  const people = $derived(
    (here?.joined ? here.members : [])
      .filter((m) => self || m.id !== here.me)
      .map((m) => ({ id: m.id, name: m.info?.name ?? `#${m.id}`, src: m.info?.avatar ?? undefined })),
  );
</script>

{#if people.length}
  <AvatarGroup {people} {max} {size} {label} class="elyra-presence {className}" {...rest} />
{/if}
