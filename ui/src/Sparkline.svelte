<!--
  A small chart of a few numbers, inline: in a Table's cell, a Stat, a tray
  popover. Plain SVG, no library. `kind` is a `line`, a filled `area`, or
  `bars`. `tone` colours it from the theme's tokens; `auto` is success when
  the last value is up on the first, danger when it's down. `label` is what
  a screen reader hears; without one it says where the numbers went.
-->
<script>
  let {
    values = [],
    kind = "line",
    tone = "accent",
    width = 80,
    height = 24,
    min = undefined,
    max = undefined,
    label = undefined,
    class: className = "",
  } = $props();

  // Only numbers count; a gap in the data isn't a zero.
  const points = $derived(values.filter((v) => typeof v === "number" && Number.isFinite(v)));
  const low = $derived(min ?? (points.length ? Math.min(...points) : 0));
  const high = $derived(max ?? (points.length ? Math.max(...points) : 0));
  // A flat series is drawn through the middle, not along an edge.
  const span = $derived(high - low || 1);
  const pad = 1.5;
  const x = (i) => (points.length === 1 ? width / 2 : pad + (i * (width - 2 * pad)) / (points.length - 1));
  const y = (v) => (high === low ? height / 2 : height - pad - ((v - low) / span) * (height - 2 * pad));

  const line = $derived(points.map((v, i) => `${i ? "L" : "M"}${x(i).toFixed(2)} ${y(v).toFixed(2)}`).join(" "));
  const area = $derived(
    points.length > 1 ? `${line} L${x(points.length - 1).toFixed(2)} ${height} L${x(0).toFixed(2)} ${height} Z` : "",
  );
  const bar = $derived(points.length ? Math.max(1, (width - 2 * pad) / points.length - 1) : 0);

  const shade = $derived(
    tone === "auto"
      ? points.length > 1 && points[points.length - 1] < points[0]
        ? "danger"
        : points.length > 1 && points[points.length - 1] > points[0]
          ? "success"
          : "muted"
      : tone,
  );
  const said = $derived(
    label ??
      (points.length === 0
        ? "No data"
        : points.length === 1
          ? `${points[0]}`
          : `From ${points[0]} to ${points[points.length - 1]}, between ${Math.min(...points)} and ${Math.max(...points)}`),
  );
</script>

<svg
  class="elyra-sparkline {shade} {className}"
  role="img"
  aria-label={said}
  {width}
  {height}
  viewBox="0 0 {width} {height}"
  preserveAspectRatio="none"
>
  {#if kind === "bars"}
    {#each points as v, i (i)}
      {@const top = Math.min(y(v), height - pad - 1)}
      <rect
        x={(pad + i * ((width - 2 * pad) / points.length)).toFixed(2)}
        y={top.toFixed(2)}
        width={bar.toFixed(2)}
        height={(height - pad - top).toFixed(2)}
        rx="0.5"
      />
    {/each}
  {:else if points.length}
    {#if kind === "area" && area}<path class="fill" d={area} />{/if}
    {#if points.length === 1}
      <circle cx={x(0)} cy={y(points[0])} r="1.5" />
    {:else}
      <path class="stroke" d={line} />
      <circle cx={x(points.length - 1)} cy={y(points[points.length - 1])} r="1.5" />
    {/if}
  {/if}
</svg>

<style>
  .elyra-sparkline { display: inline-block; overflow: visible; vertical-align: middle; color: var(--elyra-accent); }
  .success { color: var(--elyra-success); }
  .danger { color: var(--elyra-danger); }
  .warning { color: var(--elyra-warning); }
  .muted { color: var(--elyra-muted); }
  .stroke { fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linejoin: round; stroke-linecap: round; vector-effect: non-scaling-stroke; }
  .fill { fill: currentColor; opacity: 0.16; }
  circle, rect { fill: currentColor; }
</style>
