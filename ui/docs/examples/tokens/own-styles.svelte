<!-- An app's own component, on the tokens: it follows the theme with the rest. -->
<div class="invoice">
  <span class="number">INV-2041</span>
  <span class="amount">1 860 €</span>
  <span class="due">Overdue since 14 October</span>
</div>

<style>
  .invoice {
    display: grid; grid-template-columns: 1fr auto; gap: 2px 16px;
    width: 280px; padding: calc(var(--elyra-space) * 2);
    background: var(--elyra-raised);
    border: 1px solid var(--elyra-border);
    border-radius: var(--elyra-radius-lg);
    font-family: var(--elyra-font);
  }
  .number { font-family: var(--elyra-font-mono); color: var(--elyra-muted); }
  .amount { font-weight: 600; }
  .due { grid-column: 1 / -1; color: var(--elyra-danger); }
</style>
