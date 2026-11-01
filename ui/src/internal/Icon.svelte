<!-- The few icons the components need, inline: no icon font, no dependency. -->
<script>
  let { name, size = 14 } = $props();
</script>

<svg
  class="elyra-icon"
  width={size}
  height={size}
  viewBox="0 0 16 16"
  fill="none"
  stroke="currentColor"
  stroke-width="1.75"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
>
  {#if name === "chevron-down"}
    <path d="M4 6l4 4 4-4" />
  {:else if name === "chevron-left"}
    <path d="M10 4L6 8l4 4" />
  {:else if name === "chevron-right"}
    <path d="M6 4l4 4-4 4" />
  {:else if name === "calendar"}
    <rect x="2.5" y="3.5" width="11" height="10" rx="1.5" /><path d="M2.5 6.75h11M5.5 2v3M10.5 2v3" />
  {:else if name === "check"}
    <path d="M3.5 8.5l3 3 6-7" />
  {:else if name === "close"}
    <path d="M4 4l8 8M12 4l-8 8" />
  {:else if name === "minus"}
    <path d="M4 8h8" />
  {:else if name === "search"}
    <circle cx="7" cy="7" r="4.25" /><path d="M10.25 10.25L13 13" />
  {/if}
</svg>

<style>
  .elyra-icon { flex: none; display: inline-block; vertical-align: middle; }
</style>
