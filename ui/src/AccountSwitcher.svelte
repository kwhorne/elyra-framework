<!--
  The servers the user is signed in to (RFC 0022), in a menu: who they are
  where, a click to switch to another (nothing is signed out), and signing
  out of one or all. Changes that haven't reached a server yet are asked
  about before they're dropped. `onadd` adds "Add an account…": signing in
  stays the app's.

  Nothing shows while the user isn't signed in, or with a runtime before
  0.35 (no `accounts`).
-->
<script>
  // Read off the module: a runtime before 0.35 has no accounts.
  import * as runtime from "@elyra/runtime";
  import Dropdown from "./Dropdown.svelte";
  import MenuItem from "./MenuItem.svelte";
  import Separator from "./Separator.svelte";
  import Avatar from "./Avatar.svelte";
  import Icon from "./internal/Icon.svelte";

  let {
    person = undefined,
    onadd = undefined,
    variant = "ghost",
    align = "start",
    labels = {},
    class: className = "",
  } = $props();

  const words = $derived({
    signOut: (host) => `Sign out of ${host}`,
    signOutAll: "Sign out of all",
    add: "Add an account…",
    pending: (host, n) =>
      n === 1
        ? `A change made on ${host} hasn't been sent yet. Sign out and discard it?`
        : n > 1
          ? `${n} changes made on ${host} haven't been sent yet. Sign out and discard them?`
          : `Changes made on ${host} haven't been sent yet. Sign out and discard them?`,
    pendingAll: "Some changes haven't been sent yet. Sign out of every server and discard them?",
    discard: "Sign out and discard",
    waiting: (n) => `${n} waiting`,
    refused: (n) => `${n} refused`,
    attention: "Changes refused on another server",
    ...labels,
  });

  let state = $state({ accounts: [], active: null });
  $effect(() => runtime.accounts?.subscribe((s) => (state = s ?? { accounts: [], active: null })));

  const hostOf = (server) => {
    try {
      return new URL(server).host;
    } catch {
      return server;
    }
  };
  const text = (value) => (typeof value === "string" && value.trim()) || null;
  /** A name and a picture: the user's, as that server described them, unless `person` says otherwise. */
  function who(account) {
    const given = person?.(account);
    if (given) return given;
    const user = account.user ?? {};
    return {
      name: text(user.name) ?? text(user.email) ?? hostOf(account.server),
      src: text(user.avatar) ?? text(user.avatar_url) ?? text(user.profile_photo_url) ?? undefined,
    };
  }

  const list = $derived(state.accounts ?? []);
  const active = $derived(list.find((a) => a.active) ?? list[0]);
  const me = $derived(active ? who(active) : null);
  const unsent = (account) => (account.pending ?? 0) + (account.failed ?? 0);
  // A refusal on a server that isn't active: the user can't see it until they switch.
  const elsewhere = $derived(list.some((a) => a !== active && (a.failed ?? 0) > 0));

  const fail = (error) => runtime.toast?.(error?.message ?? String(error), { variant: "error" });

  async function switchTo(account) {
    if (account.active) return;
    try {
      await runtime.auth.switchTo(account.server);
    } catch (error) {
      fail(error);
    }
  }

  /** Sign out; if writes are waiting, ask, then drop them. */
  async function leave(run, discarding, question, title) {
    try {
      await run();
    } catch (error) {
      if (error?.kind !== "pending_changes") return fail(error);
      const sure = await runtime.confirm(question, { title, confirmLabel: words.discard, danger: true });
      if (!sure) return;
      try {
        await discarding();
      } catch (again) {
        fail(again);
      }
    }
  }

  const signOutOf = (account) =>
    leave(
      () => runtime.auth.signOutOf(account.server),
      () => runtime.auth.signOutOfDiscarding(account.server),
      words.pending(hostOf(account.server), unsent(account)),
      words.signOut(hostOf(account.server)),
    );
  const signOutAll = () =>
    leave(() => runtime.auth.signOutAll(), () => runtime.auth.signOutAllDiscarding(), words.pendingAll, words.signOutAll);
</script>

{#if active}
  <Dropdown {variant} {align} class="elyra-account-switcher {className}">
    {#snippet trigger()}
      <span class="face">
        <Avatar name={me.name} src={me.src} size="sm" aria-hidden="true" />
        {#if elsewhere}<span class="attention" title={words.attention} aria-hidden="true"></span>{/if}
      </span>
      <span class="who">
        <span class="name">{me.name}</span>
        {#if list.length > 1}<span class="host">{hostOf(active.server)}</span>{/if}
      </span>
      {#if elsewhere}<span class="hidden">{words.attention}</span>{/if}
    {/snippet}
    {#each list as account (account.server)}
      {@const p = who(account)}
      <MenuItem role="menuitemradio" aria-checked={account === active} onclick={() => switchTo(account)}>
        <Avatar name={p.name} src={p.src} size="sm" aria-hidden="true" />
        <span class="who">
          <span class="name">{p.name}</span>
          <span class="host">{hostOf(account.server)}</span>
          {#if unsent(account)}
            <span class="counts">
              {#if account.pending}<span>{words.waiting(account.pending)}</span>{/if}
              {#if account.failed}<span class="refused">{words.refused(account.failed)}</span>{/if}
            </span>
          {/if}
        </span>
        <span class="check" aria-hidden="true">{#if account === active}<Icon name="check" />{/if}</span>
      </MenuItem>
    {/each}
    <Separator class="elyra-account-separator" />
    {#if onadd}<MenuItem onclick={() => onadd()}>{words.add}</MenuItem>{/if}
    <MenuItem onclick={() => signOutOf(active)}>{words.signOut(hostOf(active.server))}</MenuItem>
    {#if list.length > 1}<MenuItem danger onclick={signOutAll}>{words.signOutAll}</MenuItem>{/if}
  </Dropdown>
{/if}

<style>
  .who { display: grid; min-width: 0; line-height: 1.3; text-align: left; }
  .name { overflow: hidden; color: var(--elyra-text); text-overflow: ellipsis; white-space: nowrap; }
  .host { color: var(--elyra-muted); font-size: 12px; font-weight: 400; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .counts { display: flex; flex-wrap: wrap; gap: 0 8px; color: var(--elyra-warning); font-size: 11px; font-weight: 500; }
  .refused { color: var(--elyra-danger); }
  .face { position: relative; display: inline-grid; flex: none; }
  .attention {
    position: absolute; top: -2px; right: -2px; width: 8px; height: 8px; border-radius: 50%;
    background: var(--elyra-danger); box-shadow: 0 0 0 2px var(--elyra-surface);
  }
  .hidden { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  .check { display: inline-grid; place-items: center; width: 14px; margin-left: auto; padding-left: 8px; color: var(--elyra-accent); }
  /* As wide as what it's in (a sidebar's header, which clips what's wider), the menu as wide as the button. */
  :global(.elyra-dropdown.elyra-account-switcher) { display: block; min-width: 0; max-width: 100%; }
  :global(.elyra-dropdown.elyra-account-switcher > .elyra-button) {
    width: 100%; height: auto; min-height: 32px; padding-block: 4px; text-align: left;
  }
  :global(.elyra-dropdown.elyra-account-switcher > .elyra-button > .elyra-icon:last-child) { margin-left: auto; }
  :global(.elyra-dropdown.elyra-account-switcher .menu) { right: 0; min-width: 100%; box-sizing: border-box; }
  :global(.elyra-account-separator) { margin: 4px 0; }
</style>
