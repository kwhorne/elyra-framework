<script>
  import { Badge, TreeView } from "@elyra/ui";

  // A chart of accounts (NS 4102): classes, groups, accounts.
  const accounts = [
    {
      id: "3", label: "3 Sales revenue",
      children: [
        { id: "30", label: "30 Sales, taxable", children: [{ id: "3000", label: "3000 Sales, high rate", entries: 412 }, { id: "3020", label: "3020 Sales, medium rate", entries: 18 }] },
        { id: "31", label: "31 Sales, exempt", children: [{ id: "3100", label: "3100 Sales, exempt", entries: 7 }] },
      ],
    },
    {
      id: "6", label: "6 Operating costs",
      children: [
        { id: "63", label: "63 Premises", children: [{ id: "6300", label: "6300 Rent", entries: 12 }] },
        { id: "68", label: "68 Office", children: [{ id: "6800", label: "6800 Office supplies", entries: 64 }, { id: "6860", label: "6860 Courses", entries: 3 }] },
      ],
    },
    { id: "8", label: "8 Financial items", can: "ledger.close" },
  ];

  let selected = $state("3000");
  let expanded = $state(["3", "30"]);
</script>

<div class="layout">
  <TreeView nodes={accounts} bind:selected bind:expanded label="Chart of accounts">
    {#snippet item(node)}
      <span>{node.label}</span>
      {#if node.entries}<Badge>{node.entries}</Badge>{/if}
    {/snippet}
  </TreeView>
  <p class="chosen">Account <strong>{selected}</strong> · {expanded.length} open</p>
</div>

<style>
  .layout { display: grid; gap: 8px; width: min(100%, 340px); }
  .chosen { margin: 0; color: var(--elyra-muted); font-size: 12px; }
</style>
