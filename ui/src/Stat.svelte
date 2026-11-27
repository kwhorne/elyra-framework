<!--
  A figure that matters, on a card: its `label`, its `value`, how it
  changed (`change`, a fraction: `0.12` is up 12 %), and the numbers behind
  it as a sparkline (`values`). `good` says which way is good news: `up`
  (revenue) or `down` (open tickets); the change is coloured so. `caption`
  says against what ("vs. last month").
-->
<script>
  import Sparkline from "./Sparkline.svelte";

  let {
    label,
    value,
    change = undefined,
    good = "up",
    caption = undefined,
    values = undefined,
    format = undefined,
    class: className = "",
    children,
  } = $props();

  const shown = $derived(
    format ? format(value) : typeof value === "number" ? value.toLocaleString() : (value ?? "—"),
  );
  const percent = new Intl.NumberFormat(undefined, { style: "percent", maximumFractionDigits: 1, signDisplay: "exceptZero" });
  const direction = $derived(
    typeof change !== "number" || change === 0 ? "flat" : change > 0 ? "up" : "down",
  );
  const tone = $derived(direction === "flat" ? "muted" : direction === good ? "success" : "danger");
  const arrow = $derived(direction === "up" ? "↑" : direction === "down" ? "↓" : "→");
  const said = $derived(
    typeof change === "number"
      ? `${direction === "flat" ? "Unchanged" : direction === "up" ? "Up" : "Down"} ${percent.format(Math.abs(change)).replace(/^[+−-]/, "")}${caption ? ` ${caption}` : ""}`
      : null,
  );
</script>

<div class="elyra-stat {className}" role="group" aria-label={label}>
  <div class="label">{label}</div>
  <div class="row">
    <span class="value">{shown}</span>
    {#if values?.length}<Sparkline {values} tone={tone === "muted" ? "accent" : tone} width={72} height={22} label={`${label}: ${values.join(", ")}`} />{/if}
  </div>
  {#if said}
    <div class="change {tone}" aria-label={said}>
      <span aria-hidden="true">{arrow} {percent.format(change)}</span>
      {#if caption}<span class="caption" aria-hidden="true">{caption}</span>{/if}
    </div>
  {:else if caption}
    <div class="change muted"><span class="caption">{caption}</span></div>
  {/if}
  {@render children?.()}
</div>

<style>
  .elyra-stat {
    display: grid; gap: 4px; min-width: 0; padding: 12px 14px; box-sizing: border-box;
    border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius); background: var(--elyra-raised);
    font: 13px var(--elyra-font); color: var(--elyra-text);
  }
  .label { color: var(--elyra-muted); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .row { display: flex; align-items: flex-end; justify-content: space-between; gap: 12px; }
  .value { font: 600 22px/1.15 var(--elyra-font); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .change { display: flex; flex-wrap: wrap; gap: 6px; font-size: 12px; font-variant-numeric: tabular-nums; }
  .success { color: var(--elyra-success); }
  .danger { color: var(--elyra-danger); }
  .muted { color: var(--elyra-muted); }
  .caption { color: var(--elyra-muted); }
  @media print { .elyra-stat { break-inside: avoid; background: none; } }
</style>
