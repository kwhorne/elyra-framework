<!--
  The outbox, in a drawer: the writes the server refused, with what it said
  and a way out of each, and the writes waiting for the network. A conflict
  offers Keep mine (send it again, overwriting what changed meanwhile) and
  Take theirs (drop it, and show the server's version); another refusal
  offers Send again; any write can be discarded, after asking. The
  `<StatusBar>` opens it from its counts.

  `describe(item)` names a write ("Update customer 42"); by default it's the
  method as a verb, and the path.
-->
<script>
  import { outbox as outboxStore } from "@elyra/runtime";
  import Drawer from "./Drawer.svelte";
  import Badge from "./Badge.svelte";
  import Button from "./Button.svelte";
  import Empty from "./Empty.svelte";

  let { open = $bindable(false), side = "right", title = "Changes", describe = undefined, class: className = "" } = $props();

  const own = $props.id();
  const VERBS = { POST: "Create", PUT: "Update", PATCH: "Update", DELETE: "Delete" };
  const REASONS = {
    conflict: ["Changed meanwhile", "warning"],
    validation: ["Not valid", "danger"],
    denied: ["Not allowed", "danger"],
    "not-found": ["Gone", "danger"],
  };

  const refused = $derived($outboxStore.filter((w) => w.status === "failed"));
  const waiting = $derived($outboxStore.filter((w) => w.status === "pending"));

  // The write an action is running for, and what went wrong with one.
  let busy = $state(null);
  let problem = $state({});

  const name = (w) => describe?.(w) ?? `${VERBS[w.method] ?? w.method} ${w.path}`;
  const reason = (w) => REASONS[w.reason] ?? ["Refused", "danger"];
  const saved = (w) =>
    new Date(w.createdAt).toLocaleString([], { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });

  /** The body's fields, as the user typed them: a file by its name. */
  function fields(w) {
    if (!w.body || typeof w.body !== "object" || Array.isArray(w.body)) return [];
    return Object.entries(w.body).map(([key, value]) => [key, shown(value)]);
  }
  function shown(value) {
    if (value === null || value === undefined) return "—";
    if (typeof value === "object" && typeof value.file === "string") return value.file;
    if (typeof value === "object") return JSON.stringify(value);
    return String(value);
  }
  /** The server's errors for fields the body doesn't have. */
  const otherErrors = (w) => Object.entries(w.errors ?? {}).filter(([key]) => !(w.body && key in w.body));

  async function act(w, run) {
    busy = w.id;
    problem = { ...problem, [w.id]: null };
    try {
      await run(w.id);
    } catch (error) {
      problem = { ...problem, [w.id]: error?.message ?? String(error) };
    } finally {
      busy = null;
    }
  }
</script>

<Drawer bind:open {side} {title} class="elyra-outbox {className}">
  {#if refused.length === 0 && waiting.length === 0}
    <Empty title="Nothing waiting" description="Every change has reached the server." />
  {/if}

  {#if refused.length}
    <section aria-labelledby="{own}-refused">
      <h3 id="{own}-refused">Refused <span class="count">{refused.length}</span></h3>
      <ul>
        {#each refused as w (w.id)}
          {@const [label, variant] = reason(w)}
          <li aria-label={name(w)}>
            <div class="head">
              <span class="name">{name(w)}</span>
              <Badge {variant}>{label}</Badge>
            </div>
            <!-- Laravel's 422 message repeats the first field's: the fields say it. -->
            {#if w.error && !(w.reason === "validation" && w.errors)}<p class="said">{w.error}</p>{/if}
            {#if fields(w).length || otherErrors(w).length}
              <dl>
                {#each fields(w) as [key, value] (key)}
                  <div class:invalid={w.errors?.[key]}>
                    <dt>{key}</dt>
                    <dd>
                      <span class="value">{value}</span>
                      {#each w.errors?.[key] ?? [] as message (message)}<span class="error">{message}</span>{/each}
                    </dd>
                  </div>
                {/each}
                {#each otherErrors(w) as [key, messages] (key)}
                  <div class="invalid">
                    <dt>{key}</dt>
                    <dd>{#each messages as message (message)}<span class="error">{message}</span>{/each}</dd>
                  </div>
                {/each}
              </dl>
            {/if}
            <div class="actions">
              {#if w.reason === "conflict"}
                <Button size="sm" variant="primary" loading={busy === w.id} onclick={() => act(w, outboxStore.keepMine)}>Keep mine</Button>
                <Button size="sm" disabled={busy === w.id} onclick={() => act(w, outboxStore.takeTheirs)}>Take theirs</Button>
              {:else}
                <Button size="sm" loading={busy === w.id} onclick={() => act(w, (id) => outboxStore.retry(id))}>Send again</Button>
                <Button
                  size="sm"
                  variant="danger"
                  disabled={busy === w.id}
                  confirm="Discard this change? It won't be sent."
                  confirmLabel="Discard"
                  onclick={() => act(w, outboxStore.discard)}>Discard</Button
                >
              {/if}
            </div>
            {#if problem[w.id]}<p class="problem" role="alert">{problem[w.id]}</p>{/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}

  {#if waiting.length}
    <section aria-labelledby="{own}-waiting">
      <h3 id="{own}-waiting">Waiting <span class="count">{waiting.length}</span></h3>
      <ul>
        {#each waiting as w (w.id)}
          <li aria-label={name(w)}>
            <div class="head">
              <span class="name">{name(w)}</span>
              <Button
                size="sm"
                variant="ghost"
                disabled={busy === w.id}
                confirm="Discard this change? It won't be sent."
                confirmLabel="Discard"
                onclick={() => act(w, outboxStore.discard)}>Discard</Button
              >
            </div>
            <p class="meta">
              Saved {saved(w)}{w.attempts ? ` · tried ${w.attempts === 1 ? "once" : `${w.attempts} times`}` : ""}
            </p>
            {#if problem[w.id]}<p class="problem" role="alert">{problem[w.id]}</p>{/if}
          </li>
        {/each}
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
  dl { display: grid; gap: 4px; margin: 0; font: 12px/1.45 var(--elyra-font); }
  dl > div { display: grid; grid-template-columns: minmax(0, 9em) minmax(0, 1fr); gap: 8px; }
  dt { overflow: hidden; color: var(--elyra-muted); text-overflow: ellipsis; white-space: nowrap; }
  dd { display: grid; margin: 0; min-width: 0; color: var(--elyra-text); }
  .value { overflow-wrap: anywhere; }
  .error { color: var(--elyra-danger); }
  .invalid dt { color: var(--elyra-danger); }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
</style>
