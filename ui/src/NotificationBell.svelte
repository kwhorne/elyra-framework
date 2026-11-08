<!--
  Laravel's notifications (RFC 0008), behind a bell: the unread count on it,
  and a panel with the newest, Mark all read, More, and a link to the page
  with all of them. A click on one marks it read and opens its path in the
  app (its data's `url`), or hands it to `onopen`. It follows the runtime's
  `notifications` store, so what's read on the web is read here too.

  Nothing shows in an app without the store (a runtime before notifications).
-->
<script>
  import { tick } from "svelte";
  // Read off the module: an older runtime may not have notifications.
  import * as runtime from "@elyra/runtime";
  import Icon from "./internal/Icon.svelte";
  import Empty from "./Empty.svelte";
  import { focusables, onOutside } from "./internal/focus.js";

  let {
    open = $bindable(false),
    title = "Notifications",
    href = undefined,
    present = undefined,
    onopen = undefined,
    align = "end",
    labels = {},
    class: className = "",
    ...rest
  } = $props();

  const own = $props.id();
  const words = $derived({
    unread: (n) => `${n} unread`,
    markAll: "Mark all read",
    more: "More",
    all: "See all",
    empty: "Nothing yet",
    offline: "Offline: this is the list as it was. Reading one is sent once you're back.",
    untitled: "Notification",
    justNow: "just now",
    ...labels,
  });

  const store = runtime.notifications;
  const connection = runtime.connection;
  let state = $state({ unread: 0, items: [], hasMore: false });
  $effect(() => store?.subscribe((s) => (state = s ?? { unread: 0, items: [], hasMore: false })));
  let online = $state(true);
  $effect(() => connection?.subscribe((c) => (online = c?.state !== "offline")));

  const text = (data, key) => (typeof data?.[key] === "string" && data[key].trim()) || null;
  /** Title, body and url: the data's, as the shell shows it in the OS, unless `present` says otherwise. */
  const shown = (n) =>
    present?.(n) ?? { title: text(n.data, "title") ?? words.untitled, body: text(n.data, "body"), url: text(n.data, "url") };
  const inApp = (url) => typeof url === "string" && url.startsWith("/") && !url.startsWith("//");

  const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  const STEPS = [
    ["minute", 60, 60],
    ["hour", 3600, 24],
    ["day", 86400, 7],
  ];
  /** "5 minutes ago", "yesterday"; a date past a week. */
  function when(at) {
    if (!at) return "";
    const then = new Date(at);
    const seconds = Math.round((then.getTime() - Date.now()) / 1000);
    if (Math.abs(seconds) < 60) return words.justNow;
    for (const [unit, size, upTo] of STEPS) {
      const n = Math.round(seconds / size);
      if (Math.abs(n) < upTo) return relative.format(n, unit);
    }
    return then.toLocaleDateString([], { day: "numeric", month: "short" });
  }

  let root = $state();
  let button = $state();
  let panel = $state();
  let problem = $state("");

  function close(refocus = true) {
    if (!open) return;
    open = false;
    if (refocus) button?.focus();
  }

  $effect(() => {
    if (!open) return;
    tick().then(() => (focusables(panel)[0] ?? panel)?.focus());
    return onOutside(() => [root], () => close(false));
  });

  async function attempt(work) {
    problem = "";
    try {
      await work();
    } catch (error) {
      problem = error?.message ?? String(error);
    }
  }

  async function choose(n) {
    const presented = shown(n);
    if (!n.readAt) await attempt(() => store.markRead(n.id));
    close(false);
    if (onopen) onopen(n, presented);
    else if (inApp(presented.url)) runtime.navigate(presented.url);
  }

  const count = $derived(state.unread > 99 ? "99+" : String(state.unread));
</script>

{#if store}
  <div class="elyra-notification-bell {className}" bind:this={root} {...rest}>
    <button
      bind:this={button}
      type="button"
      class="bell"
      aria-label={state.unread ? `${title}, ${words.unread(state.unread)}` : title}
      aria-haspopup="dialog"
      aria-expanded={open}
      aria-controls="{own}-panel"
      onclick={() => (open ? close() : (open = true))}
    >
      <Icon name="bell" size={16} />
      {#if state.unread}<span class="count" aria-hidden="true">{count}</span>{/if}
    </button>
    {#if open}
      <div
        bind:this={panel}
        id="{own}-panel"
        role="dialog"
        aria-labelledby="{own}-title"
        tabindex="-1"
        class="panel {align}"
        onkeydown={(event) => event.key === "Escape" && (event.stopPropagation(), close())}
      >
        <header>
          <h2 id="{own}-title">{title}</h2>
          {#if state.unread}<span class="how-many">{words.unread(state.unread)}</span>{/if}
          <button
            type="button"
            class="link"
            disabled={state.unread === 0}
            onclick={() => attempt(() => store.markAllRead())}>{words.markAll}</button
          >
        </header>
        {#if !online}<p class="offline">{words.offline}</p>{/if}
        {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
        {#if state.items.length === 0}
          <Empty title={words.empty} />
        {:else}
          <ul>
            {#each state.items as n (n.id)}
              {@const p = shown(n)}
              <li class:unread={!n.readAt}>
                <button type="button" class="item" onclick={() => choose(n)}>
                  <span class="dot" aria-hidden="true"></span>
                  <span class="text">
                    <span class="title">{p.title}</span>
                    {#if p.body}<span class="body">{p.body}</span>{/if}
                    {#if n.createdAt}
                      <time datetime={n.createdAt} title={new Date(n.createdAt).toLocaleString()}>{when(n.createdAt)}</time>
                    {/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
          {#if state.hasMore}
            <button type="button" class="link more" onclick={() => attempt(() => store.more())}>{words.more}</button>
          {/if}
        {/if}
        {#if href}
          <footer><a href={runtime.href ? runtime.href(href) : href} onclick={() => close(false)}>{words.all}</a></footer>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .elyra-notification-bell { position: relative; display: inline-block; font: 13px var(--elyra-font); }
  .bell {
    position: relative; display: inline-grid; place-items: center; width: 32px; height: 32px; padding: 0;
    border: 1px solid transparent; border-radius: var(--elyra-radius); background: transparent;
    color: var(--elyra-muted); cursor: pointer;
  }
  .bell:hover, .bell[aria-expanded="true"] { color: var(--elyra-text); background: var(--elyra-accent-soft); }
  .bell:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 2px; }
  .count {
    position: absolute; top: 2px; right: 1px; min-width: 16px; height: 16px; padding: 0 4px; box-sizing: border-box;
    border-radius: 8px; background: var(--elyra-danger); color: var(--elyra-on-accent);
    font: 600 10px/16px var(--elyra-font); font-variant-numeric: tabular-nums; text-align: center;
  }
  .panel {
    position: absolute; z-index: 60; top: calc(100% + 6px); left: 0; display: grid; gap: 8px;
    width: min(340px, calc(100vw - 24px)); max-height: min(480px, 70vh); overflow: auto; padding: 10px;
    box-sizing: border-box; background: var(--elyra-raised); color: var(--elyra-text);
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); box-shadow: var(--elyra-shadow);
    outline: none; animation: open var(--elyra-duration-fast) var(--elyra-ease);
  }
  /* Opens with a short drop into place; instant for less motion. */
  @keyframes open { from { opacity: 0; transform: translateY(-4px); } }
  .panel.end { left: auto; right: 0; }
  header { display: flex; align-items: baseline; gap: 8px; padding: 0 2px; }
  h2 { margin: 0; font: 600 13px var(--elyra-font); }
  .how-many { color: var(--elyra-muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  header .link { margin-left: auto; }
  .link {
    padding: 2px 4px; border: 0; border-radius: 4px; background: none;
    color: var(--elyra-accent); font: 500 12px var(--elyra-font); cursor: pointer;
  }
  .link:disabled { color: var(--elyra-muted); opacity: .6; cursor: default; }
  .link:focus-visible, .item:focus-visible, footer a:focus-visible { outline: 2px solid var(--elyra-focus); outline-offset: 1px; }
  .offline, .problem { margin: 0; padding: 6px 8px; border-radius: var(--elyra-radius); font-size: 12px; line-height: 1.45; }
  .offline { background: var(--elyra-inset); color: var(--elyra-muted); }
  .problem { color: var(--elyra-danger); }
  ul { display: grid; margin: 0; padding: 0; list-style: none; }
  .item {
    display: flex; align-items: flex-start; gap: 8px; width: 100%; padding: 8px; box-sizing: border-box;
    border: 0; border-radius: var(--elyra-radius); background: none; color: inherit; font: inherit;
    text-align: left; cursor: pointer;
  }
  .item:hover { background: var(--elyra-accent-soft); }
  .dot { flex: none; width: 7px; height: 7px; margin-top: 5px; border-radius: 50%; }
  .unread .dot { background: var(--elyra-accent); }
  .text { display: grid; gap: 2px; min-width: 0; }
  .title { overflow-wrap: anywhere; }
  .unread .title { font-weight: 600; }
  .body { color: var(--elyra-muted); font-size: 12px; line-height: 1.4; overflow-wrap: anywhere; }
  time { color: var(--elyra-muted); font-size: 11px; }
  .more { justify-self: center; }
  footer { padding: 6px 2px 0; border-top: 1px solid var(--elyra-border); text-align: center; }
  footer a { color: var(--elyra-accent); font-size: 12px; text-decoration: none; }
  footer a:hover { text-decoration: underline; }
</style>
