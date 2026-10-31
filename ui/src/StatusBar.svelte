<!--
  The bar at the bottom of the window, saying what the app is doing:
  offline since when, the writes waiting in the outbox (or refused), which
  open the `<OutboxDrawer>` to deal with them, the uploads and downloads
  running, and, with an app assistant, a button for it. In a debug build,
  a button for the inspector (RFC 0014), which says when it's simulating.
  Each turns off: `connection`, `outbox`, `transfers`, `assistant` and
  `inspector` (`"auto"`: shown when the app has one). Anything of the app's
  own goes in as children.
-->
<script>
  import { abilities as abilitiesStore, assistant as assistantStore, connection as connectionStore, openAssistant, outbox as outboxStore, transfers as transfersStore } from "@elyra/runtime";
  // The inspector came in 0.25: read off the module, so an older runtime
  // simply has none.
  import * as runtime from "@elyra/runtime";
  import OutboxDrawer from "./OutboxDrawer.svelte";
  import Spinner from "./Spinner.svelte";

  let { connection = true, outbox = true, transfers = true, assistant = "auto", inspector = "auto", class: className = "", children } = $props();

  let hasAssistant = $state(false);
  let outboxOpen = $state(false);
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

  const offline = $derived($connectionStore.state === "offline");
  const since = $derived(
    $connectionStore.since ? new Date($connectionStore.since).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }) : null,
  );
  const pending = $derived($outboxStore.filter((w) => w.status === "pending").length);
  const refused = $derived($outboxStore.filter((w) => w.status === "failed").length);
  const running = $derived($transfersStore.filter((t) => t.state === "running" || t.state === "queued"));
  const percent = (t) => (t.total ? Math.round((t.sent / t.total) * 100) : null);
</script>

<footer class="elyra-status-bar {className}" aria-label="Status">
  {#if connection && offline}
    <span class="item offline" role="status"><span class="dot" aria-hidden="true"></span>Offline{since ? ` since ${since}` : ""}</span>
  {/if}
  {#if outbox && pending > 0}
    <button type="button" class="item link" aria-haspopup="dialog" onclick={() => (outboxOpen = true)}>
      {pending === 1 ? "1 change waiting" : `${pending} changes waiting`}
    </button>
  {/if}
  {#if outbox && refused > 0}
    <button type="button" class="item link refused" aria-haspopup="dialog" onclick={() => (outboxOpen = true)}>
      {refused === 1 ? "1 change refused" : `${refused} changes refused`}
    </button>
  {/if}
  {#if transfers}
    {#each running as t (t.id)}
      <span class="item">
        <Spinner size="sm" label={t.direction === "upload" ? "Uploading" : "Downloading"} />
        {t.direction === "upload" ? "Uploading" : "Downloading"} {t.name}{percent(t) !== null ? ` ${percent(t)} %` : ""}
      </span>
    {/each}
  {/if}
  {@render children?.()}
  {#if hasInspector}
    <button
      type="button"
      class="inspector"
      class:simulating={simulating.length > 0}
      title={simulating.length ? `Simulating without ${simulating.join(", ")}` : "Elyra Inspector (⌘⌥E / Ctrl+Alt+E)"}
      onclick={() => runtime.openInspector?.()}
    >
      {simulating.length ? `Simulating −${simulating.length}` : "Inspector"}
    </button>
  {/if}
  {#if hasAssistant}
    <button type="button" class="assistant" onclick={() => openAssistant()}>Assistant</button>
  {/if}
</footer>
{#if outbox}<OutboxDrawer bind:open={outboxOpen} />{/if}

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
</style>
