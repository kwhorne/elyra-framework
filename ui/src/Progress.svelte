<!--
  How far along something is (`kind="progress"`: an upload, a sync, step 3 of
  5), or how full (`kind="meter"`: a quota, disk space). `value` of `max`;
  a progress with no `value` is under way for an unknown time. A meter turns
  `warning` past `low` and `danger` past `high` (where more is worse; flip
  them with `low > high` where less is). `label` is what it measures, for a
  screen reader and, with `showValue`, beside the bar.
-->
<script>
  let {
    value = undefined,
    max = 100,
    kind = "progress",
    label,
    showValue = false,
    low = undefined,
    high = undefined,
    tone = undefined,
    size = "md",
    format = undefined,
    class: className = "",
  } = $props();

  const known = $derived(typeof value === "number" && Number.isFinite(value));
  const clamped = $derived(known ? Math.min(Math.max(value, 0), max) : 0);
  const fraction = $derived(max > 0 ? clamped / max : 0);
  const percent = $derived(Math.round(fraction * 100));
  // Past a threshold, in the direction that's worse.
  const worse = (v, at) => (low !== undefined && high !== undefined && low > high ? v <= at : v >= at);
  const shade = $derived(
    tone ??
      (kind === "meter" && known
        ? high !== undefined && worse(clamped, high)
          ? "danger"
          : low !== undefined && worse(clamped, low)
            ? "warning"
            : "accent"
        : "accent"),
  );
  const text = $derived(known ? (format ? format(clamped, max) : `${percent} %`) : "");
</script>

<div class="elyra-progress {size} {className}">
  {#if showValue}
    <div class="head"><span class="label">{label}</span>{#if known}<span class="value">{text}</span>{/if}</div>
  {/if}
  <div
    class="track"
    role={kind === "meter" ? "meter" : "progressbar"}
    aria-label={label}
    aria-valuemin="0"
    aria-valuemax={max}
    aria-valuenow={known ? clamped : undefined}
    aria-valuetext={known ? text : undefined}
    aria-busy={kind === "progress" && !known ? "true" : undefined}
  >
    <div class="bar {shade}" class:indeterminate={!known} style:width={known ? `${fraction * 100}%` : undefined}></div>
  </div>
</div>

<style>
  .elyra-progress { display: grid; gap: 6px; min-width: 0; font: 12px var(--elyra-font); color: var(--elyra-muted); }
  .head { display: flex; justify-content: space-between; gap: 8px; }
  .label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .value { color: var(--elyra-text); font-variant-numeric: tabular-nums; }
  .track { position: relative; height: 6px; overflow: hidden; border-radius: 999px; background: var(--elyra-border); }
  .sm .track { height: 4px; }
  .lg .track { height: 10px; }
  .bar { height: 100%; border-radius: inherit; background: var(--elyra-accent); transition: width var(--elyra-duration) var(--elyra-ease); }
  .success { background: var(--elyra-success); }
  .warning { background: var(--elyra-warning); }
  .danger { background: var(--elyra-danger); }
  .indeterminate { position: absolute; width: 30%; animation: elyra-progress 1.2s ease-in-out infinite; }
  @keyframes elyra-progress { from { left: -30%; } to { left: 100%; } }
  @media (prefers-reduced-motion: reduce) {
    .bar { transition: none; }
    .indeterminate { left: 0; width: 100%; opacity: 0.45; animation: none; }
  }
</style>
