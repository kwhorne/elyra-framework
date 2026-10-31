<!--
  Two panes and a divider between them to drag: a list beside its record,
  an editor above its preview. `size` is the first pane's, in pixels; the
  second takes the rest. The divider is a separator the keyboard moves too,
  and `persist="key"` keeps the size in the app's settings store, so the
  window opens as the user left it.
-->
<script>
  import { untrack } from "svelte";
  import { store } from "@elyra/runtime";

  let {
    size = $bindable(280),
    direction = "horizontal",
    min = 120,
    max = Infinity,
    minSecond = 120,
    step = 16,
    persist = undefined,
    label = "Resize",
    class: className = "",
    first,
    second,
    ...rest
  } = $props();

  const own = $props.id();
  const firstId = `${own}-first`;
  const initial = untrack(() => size);
  const across = $derived(direction === "horizontal");
  const key = $derived(persist ? `elyra-ui.split.${persist}` : null);

  let root = $state();
  /** The panes' length along the divider's axis, once laid out. */
  let total = $state(0);
  let dragging = $state(false);

  const measure = () => (total = root ? (across ? root.clientWidth : root.clientHeight) : 0);
  $effect(() => {
    if (!root || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(root);
    return () => observer.disconnect();
  });

  /** The most the first pane may have: `max`, and room for the second. */
  const most = $derived(Math.max(min, Math.min(max, total > 0 ? total - minSecond : Infinity)));
  const clamp = (n) => Math.round(Math.max(min, Math.min(n, most)));

  // A window made smaller squeezes the first pane before the second.
  $effect(() => {
    const now = clamp(untrack(() => size));
    if (now !== untrack(() => size)) size = now;
  });

  // The size the user left, from the settings store (not there in a browser).
  $effect(() => {
    if (!key) return;
    let live = true;
    Promise.resolve()
      .then(() => store.get(key))
      .then((saved) => {
        if (live && typeof saved === "number" && Number.isFinite(saved)) size = clamp(saved);
      })
      .catch(() => {});
    return () => (live = false);
  });

  let timer;
  function save(wait = 0) {
    if (!key) return;
    clearTimeout(timer);
    const value = size;
    timer = setTimeout(() => Promise.resolve().then(() => store.set(key, value)).catch(() => {}), wait);
  }
  $effect(() => () => clearTimeout(timer));

  let from = 0;
  let start = 0;

  function onpointerdown(event) {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    dragging = true;
    measure();
    from = across ? event.clientX : event.clientY;
    start = size;
  }

  function onpointermove(event) {
    if (!dragging) return;
    size = clamp(start + (across ? event.clientX : event.clientY) - from);
  }

  function onpointerup(event) {
    if (!dragging) return;
    dragging = false;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    save();
  }

  function onkeydown(event) {
    const by = event.shiftKey ? step * 4 : step;
    const keys = across
      ? { ArrowLeft: -by, ArrowRight: by }
      : { ArrowUp: -by, ArrowDown: by };
    let next;
    if (event.key in keys) next = size + keys[event.key];
    else if (event.key === "Home") next = min;
    else if (event.key === "End") next = most;
    else if (event.key === "Enter") next = initial;
    else return;
    event.preventDefault();
    measure();
    size = clamp(next);
    save(300);
  }

  function ondblclick() {
    size = clamp(initial);
    save();
  }
</script>

<div
  class="elyra-split-pane {direction} {className}"
  class:dragging
  bind:this={root}
  {...rest}
>
  <div class="pane first" id={firstId} style:flex-basis="{size}px">
    {@render first?.()}
  </div>
  <!-- A focusable separator is a control (a window splitter): it takes the keys. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    class="divider"
    role="separator"
    tabindex="0"
    aria-label={label}
    aria-controls={firstId}
    aria-orientation={across ? "vertical" : "horizontal"}
    aria-valuenow={size}
    aria-valuemin={min}
    aria-valuemax={Number.isFinite(most) ? most : undefined}
    {onpointerdown}
    {onpointermove}
    {onpointerup}
    onpointercancel={onpointerup}
    {onkeydown}
    {ondblclick}
  ></div>
  <div class="pane second">
    {@render second?.()}
  </div>
</div>

<style>
  .elyra-split-pane { display: flex; width: 100%; height: 100%; min-width: 0; min-height: 0; overflow: hidden; }
  .vertical { flex-direction: column; }
  .pane { min-width: 0; min-height: 0; overflow: auto; }
  .first { flex-grow: 0; flex-shrink: 0; }
  .second { flex: 1 1 0; }
  .divider { position: relative; flex: none; background: var(--elyra-border); touch-action: none; outline: none; }
  .horizontal > .divider { width: 1px; cursor: col-resize; }
  .vertical > .divider { height: 1px; cursor: row-resize; }
  /* A wider grip than the line it draws. */
  .divider::before { content: ""; position: absolute; z-index: 1; }
  .horizontal > .divider::before { inset: 0 -3px; }
  .vertical > .divider::before { inset: -3px 0; }
  .divider:hover, .dragging > .divider, .divider:focus-visible { background: var(--elyra-accent); }
  .horizontal > .divider:hover, .horizontal.dragging > .divider, .horizontal > .divider:focus-visible { box-shadow: 0 0 0 1px var(--elyra-accent); }
  .vertical > .divider:hover, .vertical.dragging > .divider, .vertical > .divider:focus-visible { box-shadow: 0 0 0 1px var(--elyra-accent); }
  .dragging { user-select: none; }
  .horizontal.dragging { cursor: col-resize; }
  .vertical.dragging { cursor: row-resize; }
</style>
