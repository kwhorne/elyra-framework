<!--
  Who else has this open (RFC 0015): the other members of a Laravel
  presence channel, as an AvatarGroup. It joins `channel` while it's on
  screen and leaves when it goes. Nothing shows while nobody else is
  there, offline, or in an app without presence (or an older runtime).

  `editing` (an edit form's "changed") tells the others the user is
  editing it; someone else who is gets a ring and "Ada is editing". It's
  what each says, not a lock.
-->
<script>
  // Read off the module: a runtime from before 0.26 has no presence.
  import * as runtime from "@elyra/runtime";
  import AvatarGroup from "./AvatarGroup.svelte";

  let {
    channel,
    editing = false,
    self = false,
    max = 4,
    size = "sm",
    label = "Also here",
    labels = {},
    class: className = "",
    ...rest
  } = $props();

  const words = $derived({
    editing: (names) =>
      names.length === 1
        ? `${names[0]} is editing`
        : names.length === 2
          ? `${names[0]} and ${names[1]} are editing`
          : `${names.length} people are editing`,
    note: "editing",
    ...labels,
  });

  let here = $state(null);
  // One store per channel name: props updated around it don't join again.
  let made = null;
  const store = $derived.by(() => {
    if (made?.channel !== channel) made = { channel, store: runtime.presence?.(channel) };
    return made.store;
  });
  $effect(() => {
    if (!store) return;
    return store.subscribe((state) => (here = state));
  });
  // Said after the subscribe above has joined; a runtime before 0.29 can't.
  $effect(() => {
    store?.editing?.(editing);
  });

  const nameOf = (m) => m.info?.name ?? `#${m.id}`;
  const members = $derived(here?.joined ? here.members : []);
  const people = $derived(
    members
      .filter((m) => self || m.id !== here.me)
      .map((m) => ({ id: m.id, name: nameOf(m), src: m.info?.avatar ?? undefined, note: m.editing ? words.note : undefined })),
  );
  // The others editing it: the user knows when they are.
  const editors = $derived(members.filter((m) => m.editing && m.id !== here.me).map(nameOf));
</script>

{#if people.length}
  <span class="elyra-presence-wrap">
    <AvatarGroup {people} {max} {size} {label} class="elyra-presence {className}" {...rest} />
    <span class="editing" aria-live="polite">
      {#if editors.length}<span aria-hidden="true">✎</span> {words.editing(editors)}{/if}
    </span>
  </span>
{/if}

<style>
  .elyra-presence-wrap { display: inline-flex; align-items: center; gap: 8px; min-width: 0; }
  .editing { font: 12px/1.3 var(--elyra-font); color: var(--elyra-muted); white-space: nowrap; }
  .editing:empty { display: none; }
</style>
