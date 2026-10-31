<!-- Where content will be, while it loads: `lines` of text, or one block. -->
<script>
  let { lines = 1, width = "100%", height = "12px", class: className = "" } = $props();
</script>

<span class="elyra-skeleton {className}" aria-hidden="true">
  {#each { length: lines } as _, i}
    <span class="line" style:width={i === lines - 1 && lines > 1 ? "60%" : width} style:height></span>
  {/each}
</span>

<style>
  .elyra-skeleton { display: grid; gap: 8px; }
  .line {
    display: block; border-radius: 4px;
    background: linear-gradient(90deg, var(--elyra-inset) 25%, color-mix(in srgb, var(--elyra-inset) 60%, var(--elyra-border)) 50%, var(--elyra-inset) 75%);
    background-size: 200% 100%; animation: elyra-shimmer 1.4s ease-in-out infinite;
  }
  @keyframes elyra-shimmer { from { background-position: 200% 0; } to { background-position: -200% 0; } }
  @media (prefers-reduced-motion: reduce) { .line { animation: none; } }
</style>
