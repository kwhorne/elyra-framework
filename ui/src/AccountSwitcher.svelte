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
    pending: (host) => `Changes made on ${host} haven't been sent yet. Sign out and discard them?`,
    pendingAll: "Some changes haven't been sent yet. Sign out of every server and discard them?",
    discard: "Sign out and discard",
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
      words.pending(hostOf(account.server)),
      words.signOut(hostOf(account.server)),
    );
  const signOutAll = () =>
    leave(() => runtime.auth.signOutAll(), () => runtime.auth.signOutAllDiscarding(), words.pendingAll, words.signOutAll);
</script>

{#if active}
  <Dropdown {variant} {align} class="elyra-account-switcher {className}">
    {#snippet trigger()}
      <Avatar name={me.name} src={me.src} size="sm" aria-hidden="true" />
      <span class="who">
        <span class="name">{me.name}</span>
        {#if list.length > 1}<span class="host">{hostOf(active.server)}</span>{/if}
      </span>
    {/snippet}
    {#each list as account (account.server)}
      {@const p = who(account)}
      <MenuItem role="menuitemradio" aria-checked={account === active} onclick={() => switchTo(account)}>
        <Avatar name={p.name} src={p.src} size="sm" aria-hidden="true" />
        <span class="who">
          <span class="name">{p.name}</span>
          <span class="host">{hostOf(account.server)}</span>
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
