<!--
  The files open in the user's own apps (RFC 0038), in a drawer: each one's
  name and where its last save is, with what to do. A save the server turned
  down because the file changed there too offers Keep mine and Take theirs;
  any file can be opened in its app again, shown in its folder, or finished
  with. Finishing one whose last save hasn't reached the server asks first,
  since that save would be lost. The `<StatusBar>` opens it.
-->
<script>
  import { confirm } from "@elyra/runtime";
  // Editing came in 0.50: read off the module, so an older runtime has an
  // empty drawer rather than a build that fails.
  import * as runtime from "@elyra/runtime";
  import Drawer from "./Drawer.svelte";
  import Badge from "./Badge.svelte";
  import Button from "./Button.svelte";
  import Empty from "./Empty.svelte";
  import { useWords } from "./internal/words.js";

  let { open = $bindable(false), side = "right", title = undefined, labels = {}, class: className = "" } = $props();

  const words = useWords("editing", () => labels);
  const own = $props.id();
  const none = { subscribe: (run) => (run([]), () => {}) };
  const editingStore = runtime.editing ?? none;
  // What each state is called, and how loud.
  const STATES = {
    open: "neutral",
    saving: "accent",
    saved: "success",
    waiting: "warning",
    conflict: "warning",
    denied: "danger",
    failed: "danger",
  };
  const NEEDS_YOU = ["conflict", "denied", "failed"];

  const files = $derived($editingStore ?? []);
  const needsYou = $derived(files.filter((f) => NEEDS_YOU.includes(f.state)));
  const rest = $derived(files.filter((f) => !NEEDS_YOU.includes(f.state)));

  // The file an action is running for, and what went wrong with one.
  let busy = $state(null);
  let problem = $state({});

  const shownAs = (f) => (STATES[f.state] ? [words[f.state], STATES[f.state]] : [f.state, "neutral"]);
  const savedAt = (f) =>
    f.savedAt ? new Date(f.savedAt).toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : null;
  // Finder on a Mac, the file manager elsewhere.
  const onMac = /Mac/.test(globalThis.navigator?.platform ?? "");

  async function act(f, run) {
    busy = f.id;
    problem = { ...problem, [f.id]: null };
    try {
      await run(f.id);
    } catch (error) {
      problem = { ...problem, [f.id]: error?.message ?? String(error) };
    } finally {
      busy = null;
    }
  }

  /** Done with it; a save still on its way is the user's to give up. */
  async function finish(f) {
    busy = f.id;
    problem = { ...problem, [f.id]: null };
    try {
      await editingStore.finish(f.id);
    } catch (error) {
      if (error?.kind !== "pending_changes") {
        problem = { ...problem, [f.id]: error?.message ?? String(error) };
        return;
      }
      const sure = await confirm(words.unsaved(f.name), {
        danger: true,
        confirmLabel: words.stop,
      });
      if (sure) await act(f, (id) => editingStore.finish(id, { discard: true }));
    } finally {
      busy = null;
    }
  }
</script>

{#snippet file(f)}
  {@const [label, variant] = shownAs(f)}
  <li aria-label={f.name}>
    <div class="head">
      <span class="name">{f.name}</span>
      <Badge {variant}>{label}</Badge>
    </div>
    {#if f.error}<p class="said">{f.error}</p>{/if}
    {#if savedAt(f)}<p class="meta">{words.savedAt(savedAt(f))}</p>{/if}
    <div class="actions">
      {#if f.state === "conflict"}
        <Button size="sm" variant="primary" loading={busy === f.id} onclick={() => act(f, editingStore.keepMine)}>{words.keepMine}</Button>
        <Button size="sm" disabled={busy === f.id} onclick={() => act(f, editingStore.takeTheirs)}>{words.takeTheirs}</Button>
      {/if}
      <Button size="sm" variant="ghost" disabled={busy === f.id} onclick={() => act(f, editingStore.show)}>{words.show}</Button>
      <Button size="sm" variant="ghost" disabled={busy === f.id} onclick={() => act(f, editingStore.reveal)}>{onMac ? words.showInFinder : words.showInFolder}</Button>
      <Button size="sm" variant="ghost" disabled={busy === f.id} onclick={() => finish(f)}>{words.done}</Button>
    </div>
    {#if problem[f.id]}<p class="problem" role="alert">{problem[f.id]}</p>{/if}
  </li>
{/snippet}

<Drawer bind:open {side} title={title ?? words.title} class="elyra-editing {className}">
  {#if files.length === 0}
    <Empty title={words.emptyTitle} description={words.emptyDescription} />
  {/if}

  {#if needsYou.length}
    <section aria-labelledby="{own}-needs">
      <h3 id="{own}-needs">{words.needsYou} <span class="count">{needsYou.length}</span></h3>
      <ul>
        {#each needsYou as f (f.id)}{@render file(f)}{/each}
      </ul>
    </section>
  {/if}

  {#if rest.length}
    <section aria-labelledby="{own}-open">
      <h3 id="{own}-open">{words.openHeading} <span class="count">{rest.length}</span></h3>
      <ul>
        {#each rest as f (f.id)}{@render file(f)}{/each}
      </ul>
    </section>
  {/if}
</Drawer>

<style>
  section { display: grid; gap: 8px; }
  section + section { margin-top: 20px; }
  h3 {
    display: flex; align-items: center; gap: 6px; margin: 0;
    color: var(--elyra-muted); font: 600 11px var(--elyra-font); letter-spacing: 0.04em; text-transform: uppercase;
  }
  .count { font-variant-numeric: tabular-nums; }
  ul { display: grid; gap: 8px; margin: 0; padding: 0; list-style: none; }
  li {
    display: grid; gap: 8px; padding: 10px 12px;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius); background: var(--elyra-surface);
  }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; }
  .name { min-width: 0; overflow-wrap: anywhere; color: var(--elyra-text); font: 600 13px var(--elyra-font); }
  .said, .meta, .problem { margin: 0; font: 12px/1.45 var(--elyra-font); }
  .said { color: var(--elyra-text); }
  .meta { color: var(--elyra-muted); font-variant-numeric: tabular-nums; }
  .problem { color: var(--elyra-danger); }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
</style>
