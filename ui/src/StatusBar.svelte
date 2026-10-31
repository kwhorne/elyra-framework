<!--
  The bar at the bottom of the window, saying what the app is doing:
  offline since when, the writes waiting in the outbox (or refused), the
  uploads and downloads running, and, with an app assistant, a button for
  it. Each turns off: `connection`, `outbox`, `transfers`, `assistant`
  (`"auto"`: shown when the app has one). Anything of the app's own goes in
  as children.
-->
<script>
  import { assistant as assistantStore, connection as connectionStore, openAssistant, outbox as outboxStore, transfers as transfersStore } from "@elyra/runtime";
  import Spinner from "./Spinner.svelte";

  let { connection = true, outbox = true, transfers = true, assistant = "auto", class: className = "", children } = $props();

  let hasAssistant = $state(false);
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
    <span class="item">{pending === 1 ? "1 change waiting" : `${pending} changes waiting`}</span>
  {/if}
  {#if outbox && refused > 0}
    <span class="item refused">{refused === 1 ? "1 change refused" : `${refused} changes refused`}</span>
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
  {#if hasAssistant}
    <button type="button" class="assistant" onclick={() => openAssistant()}>Assistant</button>
  {/if}
</footer>

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
  .assistant {
    margin-left: auto; height: 20px; padding: 0 8px; border: 1px solid var(--elyra-border); border-radius: 999px;
    background: transparent; color: var(--elyra-text); font: inherit; cursor: pointer;
  }
  .assistant:hover { border-color: var(--elyra-accent); color: var(--elyra-accent); }
  .assistant:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
</style>
