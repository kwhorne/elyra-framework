<!--
  The bar at the bottom of the window, saying what the app is doing:
  offline since when, the writes waiting in the outbox (or refused), which
  open the `<OutboxDrawer>` to deal with them, the uploads and downloads
  running, the files open in their own apps (RFC 0038), which open the
  `<EditingDrawer>`, and, with an app assistant, a button for it. In a debug build,
  a button for the inspector (RFC 0014), which says when it's simulating:
  offline, or without abilities.
  Each turns off: `connection`, `outbox`, `transfers`, `editing`,
  `assistant` and `inspector` (`"auto"`: shown when the app has one). Anything of the app's
  own goes in as children.
-->
<script>
  import { abilities as abilitiesStore, assistant as assistantStore, connection as connectionStore, openAssistant, outbox as outboxStore, transfers as transfersStore } from "@elyra/runtime";
  // The inspector came in 0.24, editing in 0.50: read off the module, so an
  // older runtime simply has neither.
  import * as runtime from "@elyra/runtime";
  import OutboxDrawer from "./OutboxDrawer.svelte";
  import EditingDrawer from "./EditingDrawer.svelte";
  import Spinner from "./Spinner.svelte";
  import { useWords } from "./internal/words.js";

  let { connection = true, outbox = true, transfers = true, editing = true, assistant = "auto", inspector = "auto", labels = {}, class: className = "", children } = $props();
  const words = useWords("statusbar", () => labels);

  let hasAssistant = $state(false);
  let outboxOpen = $state(false);
  let editingOpen = $state(false);
  // The editings, followed only while shown.
  let files = $state([]);
  $effect(() => {
    if (!editing || !runtime.editing) return;
    return runtime.editing.subscribe((list) => (files = list ?? []));
  });
  const needYou = $derived(files.filter((f) => f.state === "conflict" || f.state === "denied" || f.state === "failed").length);
  const savingNow = $derived(files.find((f) => f.state === "saving"));
  const waitingSaves = $derived(files.filter((f) => f.state === "waiting").length);
  $effect(() => {
    if (assistant !== "auto") {
      hasAssistant = assistant === true;
      return;
    }
    let live = true;
    assistantStore.source().then(
      () => live && (hasAssistant = true),
      () => live && (hasAssistant = false),
    );
    return () => (live = false);
  });

  let hasInspector = $state(false);
  $effect(() => {
    if (inspector !== "auto") {
      hasInspector = inspector === true;
      return;
    }
    let live = true;
    Promise.resolve()
      .then(() => runtime.inspector?.available?.() ?? false)
      .then(
        (yes) => live && (hasInspector = yes === true),
        () => live && (hasInspector = false),
      );
    return () => (live = false);
  });
  const simulating = $derived($abilitiesStore?.masked ?? []);
  // Unplugged in the inspector: offline, but not for real.
  const unplugged = $derived($connectionStore.simulated === true);
  const simulatingWhat = $derived(
    [unplugged ? "offline" : null, simulating.length ? `−${simulating.length}` : null].filter(Boolean).join(" "),
  );
  const simulatingTitle = $derived(
    [unplugged ? "offline" : null, simulating.length ? `without ${simulating.join(", ")}` : null].filter(Boolean).join(", "),
  );

  const offline = $derived($connectionStore.state === "offline");
  const since = $derived(
    $connectionStore.since ? new Date($connectionStore.since).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : null,
  );
  const pending = $derived($outboxStore.filter((w) => w.status === "pending").length);
  const refused = $derived($outboxStore.filter((w) => w.status === "failed").length);
  const running = $derived($transfersStore.filter((t) => t.state === "running" || t.state === "queued"));
  const percent = (t) => (t.total ? Math.round((t.sent / t.total) * 100) : null);
</script>

<footer class="elyra-status-bar {className}" aria-label={words.label}>
  {#if connection && offline}
    <span class="item offline" role="status"><span class="dot" aria-hidden="true"></span>{unplugged ? words.offlineSimulated : since ? words.offlineSince(since) : words.offline}</span>
  {/if}
  {#if outbox && pending > 0}
    <button type="button" class="item link" aria-haspopup="dialog" onclick={() => (outboxOpen = true)}>
      {words.pending(pending)}
    </button>
  {/if}
  {#if outbox && refused > 0}
    <button type="button" class="item link refused" aria-haspopup="dialog" onclick={() => (outboxOpen = true)}>
      {words.refused(refused)}
    </button>
  {/if}
  {#if transfers}
    {#each running as t (t.id)}
      <span class="item">
        <Spinner size="sm" label={t.direction === "upload" ? words.uploading : words.downloading} />
        {t.direction === "upload" ? words.uploadingName(t.name) : words.downloadingName(t.name)}{percent(t) !== null ? ` ${percent(t)} %` : ""}
      </span>
    {/each}
  {/if}
  {#if editing && files.length > 0}
    {#if needYou > 0}
      <button type="button" class="item link refused" aria-haspopup="dialog" onclick={() => (editingOpen = true)}>
        {words.needYou(needYou)}
      </button>
    {:else if savingNow}
      <button type="button" class="item link" aria-haspopup="dialog" onclick={() => (editingOpen = true)}>
        <Spinner size="sm" label={words.saving} />{words.savingName(savingNow.name)}
      </button>
    {:else if waitingSaves > 0}
      <button type="button" class="item link waiting" aria-haspopup="dialog" onclick={() => (editingOpen = true)}>
        {words.waitingSaves(waitingSaves)}
      </button>
    {:else}
      <button type="button" class="item link" aria-haspopup="dialog" onclick={() => (editingOpen = true)}>
        {files.length === 1 ? words.editingOne(files[0].name) : words.editingMany(files.length)}
      </button>
    {/if}
  {/if}
  {@render children?.()}
  {#if hasInspector}
    <button
      type="button"
      class="inspector"
      class:simulating={simulatingWhat !== ""}
      title={simulatingWhat ? `Simulating ${simulatingTitle}` : "Elyra Inspector (⌘⌥E / Ctrl+Alt+E)"}
      onclick={() => runtime.openInspector?.()}
    >
      {simulatingWhat ? `Simulating ${simulatingWhat}` : "Inspector"}
    </button>
  {/if}
  {#if hasAssistant}
    <button type="button" class="assistant" onclick={() => openAssistant()}>{words.assistant}</button>
  {/if}
</footer>
{#if outbox}<OutboxDrawer bind:open={outboxOpen} />{/if}
{#if editing && runtime.editing}<EditingDrawer bind:open={editingOpen} />{/if}

<style>
  .elyra-status-bar {
    display: flex; align-items: center; gap: 16px; min-height: 26px; padding: 0 12px; box-sizing: border-box; flex: none;
    background: var(--elyra-surface); border-top: 1px solid var(--elyra-border);
    color: var(--elyra-muted); font: 12px var(--elyra-font); font-variant-numeric: tabular-nums;
  }
  .item { display: inline-flex; align-items: center; gap: 6px; white-space: nowrap; }
  .offline { color: var(--elyra-warning); }
  .dot { width: 7px; height: 7px; border-radius: 50%; background: currentColor; }
  .refused { color: var(--elyra-danger); }
  .waiting { color: var(--elyra-warning); }
  .link { padding: 0; border: 0; background: none; color: inherit; font: inherit; cursor: pointer; }
  .link:hover { text-decoration: underline; text-underline-offset: 2px; }
  .link:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; border-radius: 2px; }
  .inspector {
    margin-left: auto; height: 20px; padding: 0 8px; border: 1px dashed var(--elyra-border); border-radius: 999px;
    background: transparent; color: var(--elyra-muted); font: inherit; cursor: pointer;
  }
  .inspector.simulating { border-style: solid; border-color: var(--elyra-warning); color: var(--elyra-warning); }
  .inspector:hover { color: var(--elyra-text); }
  .inspector:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  .inspector + .assistant { margin-left: 0; }
  .assistant {
    margin-left: auto; height: 20px; padding: 0 8px; border: 1px solid var(--elyra-border); border-radius: 999px;
    background: transparent; color: var(--elyra-text); font: inherit; cursor: pointer;
  }
  .assistant:hover { border-color: var(--elyra-accent); color: var(--elyra-accent); }
  .assistant:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  @media print { .elyra-status-bar { display: none; } }
</style>
