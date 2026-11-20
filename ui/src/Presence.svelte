<!--
  Who else has this open (RFC 0015): the other members of a Laravel
  presence channel, as an AvatarGroup. It joins `channel` while it's on
  screen and leaves when it goes. Nothing shows while nobody else is
  there, offline, or in an app without presence (or an older runtime).

  `editing` (an edit form's "changed") tells the others the user is
  editing it; someone else who is gets a ring and "Ada is editing". It's
  what each says, not a lock.

  `follow` (RFC 0042) adds Follow…, a menu of the others: the one chosen is
  asked, and once they allow it this window goes where theirs goes, with
  "Following Ada" and Stop beside the avatars. Someone following the user
  shows as "Grete is following you", with Stop.
-->
<script>
  // Read off the module: a runtime from before 0.26 has no presence.
  import * as runtime from "@elyra/runtime";
  import AvatarGroup from "./AvatarGroup.svelte";
  import Button from "./Button.svelte";
  import Dropdown from "./Dropdown.svelte";
  import MenuItem from "./MenuItem.svelte";

  let {
    channel,
    editing = false,
    follow = false,
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
    follow: "Follow…",
    following: (name) => `Following ${name}`,
    away: (name) => `${name} is somewhere you can't follow`,
    asking: (name) => `Asking ${name}…`,
    followedBy: (names) => (names.length === 1 ? `${names[0]} is following you` : `${names.length} people are following you`),
    stop: "Stop",
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

  // Following (RFC 0042); a runtime before it has none.
  let followingNow = $state(null);
  let followedBy = $state([]);
  let asking = $state(null);
  $effect(() => {
    if (!follow || !runtime.following) return;
    const a = runtime.following.subscribe((f) => (followingNow = f));
    const b = runtime.followers.subscribe((f) => (followedBy = f));
    return () => { a(); b(); };
  });
  const byId = (id) => members.find((m) => m.id === id);
  const leader = $derived(followingNow?.channel === channel ? byId(followingNow.leader) : null);
  const mine = $derived(followedBy.filter((f) => f.channel === channel).map((f) => byId(f.id)).filter(Boolean).map(nameOf));
  const others = $derived(members.filter((m) => m.id !== here?.me));
  async function ask(m) {
    asking = nameOf(m);
    try {
      await store?.follow?.(m.id);
    } finally {
      asking = null;
    }
  }
</script>

{#if people.length}
  <span class="elyra-presence-wrap">
    <AvatarGroup {people} {max} {size} {label} class="elyra-presence {className}" {...rest} />
    <span class="editing" aria-live="polite">
      {#if editors.length}<span aria-hidden="true">✎</span> {words.editing(editors)}{/if}
    </span>
    {#if follow && runtime.following}
      <span class="follow" aria-live="polite">
        {#if leader}
          <span>{followingNow.away ? words.away(nameOf(leader)) : words.following(nameOf(leader))}</span>
          <Button size="sm" variant="ghost" onclick={() => runtime.following.stop()}>{words.stop}</Button>
        {:else if asking}
          <span>{words.asking(asking)}</span>
        {:else if others.length}
          <Dropdown label={words.follow} size="sm" variant="ghost" align="end">
            {#each others as m (m.id)}
              <MenuItem onclick={() => ask(m)}>{nameOf(m)}</MenuItem>
            {/each}
          </Dropdown>
        {/if}
        {#if mine.length}
          <span>{words.followedBy(mine)}</span>
          <Button size="sm" variant="ghost" onclick={() => runtime.following.stopFollowers()}>{words.stop}</Button>
        {/if}
      </span>
    {/if}
  </span>
{/if}

<style>
  .elyra-presence-wrap { display: inline-flex; align-items: center; gap: 8px; min-width: 0; }
  .editing { font: 12px/1.3 var(--elyra-font); color: var(--elyra-muted); white-space: nowrap; }
  .editing:empty { display: none; }
  .follow { display: inline-flex; align-items: center; gap: 6px; font: 12px/1.3 var(--elyra-font); color: var(--elyra-muted); white-space: nowrap; }
  .follow:empty { display: none; }
</style>
