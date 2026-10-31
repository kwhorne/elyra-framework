<!-- One theme's column: every component, on that theme's tokens. -->
<script>
  import { ValidationError } from "@elyra/runtime";
  import {
    Button, ButtonGroup, Dropdown, MenuItem, Form, Field, Input, Textarea, Select,
    Checkbox, Switch, RadioGroup, Modal, Drawer, Tooltip, Popover,
    Table, Badge, Tabs, Card, Callout, Empty, Spinner, Skeleton, Separator, Toolbar, StatusBar, NavItem, Sidebar,
  } from "../src/index.js";

  let { theme } = $props();

  const COUNTRIES = ["Denmark", "Finland", "Iceland", "Norway", "Sweden", "Estonia", "Latvia", "Lithuania", "Germany", "Netherlands"];
  let name = $state("Ada Lovelace");
  let email = $state("ada@");
  let notes = $state("Prefers invoices by email.");
  let country = $state("Norway");
  let team = $state(undefined);
  let vip = $state(true);
  let active = $state(true);
  let plan = $state("yearly");
  let modal = $state(false);
  let drawer = $state(false);
  let saved = $state("");
  let query = $state({});
  let tab = $state("invoices");
  const CUSTOMERS = [
    { id: 1, name: "Ada Lovelace", email: "ada@analytical.co", country: "UK", credit: 12400, status: "Active" },
    { id: 2, name: "Grace Hopper", email: "grace@cobol.dev", country: "US", credit: 860, status: "Overdue" },
    { id: 3, name: "Linus Torvalds", email: "linus@kernel.org", country: "Finland", credit: 3150, status: "Active" },
    { id: 4, name: "Margaret Hamilton", email: "margaret@apollo.space", country: "US", credit: 22000, status: "Paused" },
    { id: 5, name: "Edsger Dijkstra", email: "edsger@goto.nl", country: "Netherlands", credit: 0, status: "Active" },
    { id: 6, name: "Barbara Liskov", email: "barbara@clu.mit.edu", country: "US", credit: 4700, status: "Active" },
  ];
  const STATUS = { Active: "success", Overdue: "danger", Paused: "warning" };
  const money = (n) => new Intl.NumberFormat("en", { style: "currency", currency: "EUR", maximumFractionDigits: 0 }).format(n);

  async function save() {
    saved = "";
    await new Promise((r) => setTimeout(r, 700));
    if (!email.includes(".")) {
      throw new ValidationError("customers_store", { email: ["The email must be a valid email address."] }, "{}");
    }
    saved = "Saved.";
  }
</script>

<section class="column theme-{theme}" aria-label="{theme} theme">
  <h2>{theme === "dark" ? "Dark" : "Light"}</h2>

  <div class="block">
    <h3>Buttons</h3>
    <div class="row">
      <Button variant="primary">New customer</Button>
      <Button>Export</Button>
      <Button variant="ghost">Cancel</Button>
      <Button variant="danger" confirm="Delete Ada Lovelace? This can't be undone." confirmLabel="Delete">Delete</Button>
    </div>
    <div class="row">
      <Button size="sm">Small</Button>
      <Button loading>Saving</Button>
      <Button disabled>Disabled</Button>
      <Button can="customers.delete" canMode="disable" variant="danger">No ability</Button>
    </div>
    <div class="row">
      <ButtonGroup label="View">
        <Button size="sm">List</Button><Button size="sm">Board</Button><Button size="sm">Calendar</Button>
      </ButtonGroup>
      <Dropdown label="Actions" size="sm">
        <MenuItem>Edit</MenuItem>
        <MenuItem>Duplicate</MenuItem>
        <MenuItem disabled>Archive</MenuItem>
        <MenuItem can="customers.delete" canMode="disable" danger>Delete</MenuItem>
      </Dropdown>
    </div>
  </div>

  <div class="block">
    <h3>A form, with Laravel's errors</h3>
    <Form onsubmit={save}>
      <Field name="name" label="Name" required description="As it appears on invoices.">
        <Input bind:value={name} />
      </Field>
      <Field name="email" label="Email" required>
        <Input type="email" bind:value={email} placeholder="name@example.com" />
      </Field>
      <div class="two">
        <Field label="Country"><Select bind:value={country} options={COUNTRIES} /></Field>
        <Field label="Team"><Select bind:value={team} options={["Sales", "Support", { value: "ops", label: "Operations", disabled: true }]} placeholder="No team" /></Field>
      </div>
      <Field label="Notes"><Textarea bind:value={notes} rows={2} /></Field>
      <Field label="Plan"><RadioGroup bind:value={plan} inline options={[{ value: "monthly", label: "Monthly" }, { value: "yearly", label: "Yearly" }]} /></Field>
      <div class="row">
        <Checkbox bind:checked={vip}>VIP customer</Checkbox>
        <Switch bind:checked={active}>Active</Switch>
      </div>
      <div class="row end">
        {#if saved}<span class="note">{saved}</span>{/if}
        <Button type="submit" variant="primary">Save customer</Button>
      </div>
    </Form>
  </div>

  <div class="block">
    <h3>Overlays</h3>
    <div class="row">
      <Button onclick={() => (modal = true)}>Open a modal</Button>
      <Button onclick={() => (drawer = true)}>Open a drawer</Button>
      <Tooltip text="Copies the customer's link"><Button variant="ghost">Copy link</Button></Tooltip>
      <Popover label="Filter">
        <div class="stack">
          <Field label="Status"><Select options={["Active", "Paused", "Closed"]} value="Active" /></Field>
          <Checkbox checked>Only VIP</Checkbox>
        </div>
      </Popover>
    </div>
  </div>

  <div class="block">
    <h3>A table, searched, sorted and paged</h3>
    <Table
      rows={CUSTOMERS}
      bind:query
      perPage={4}
      search
      sortable={["name", "credit"]}
      label="Customers"
      columns={["name", "email", { key: "credit", label: "Credit", align: "end", format: money }, { key: "status", label: "Status" }]}
      onrowclick={() => {}}
    >
      {#snippet cell(row, col)}
        {#if col.key === "status"}<Badge variant={STATUS[row.status]}>{row.status}</Badge>
        {:else if col.key === "credit"}{money(row.credit)}
        {:else}{row[col.key]}{/if}
      {/snippet}
      {#snippet actions(row)}
        <Dropdown label="More" size="sm" variant="ghost" align="end">
          <MenuItem>Open {row.name.split(" ")[0]}</MenuItem>
          <MenuItem can="customers.delete" canMode="disable" danger>Delete</MenuItem>
        </Dropdown>
      {/snippet}
    </Table>
  </div>

  <Card title="Ada Lovelace" description="Customer since March 2021 · Analytical Engines Ltd">
    {#snippet actions()}<Badge variant="success">Active</Badge><Button size="sm">Edit</Button>{/snippet}
    <Tabs bind:value={tab} label="Customer" tabs={[{ id: "details", label: "Details" }, { id: "invoices", label: "Invoices" }, { id: "notes", label: "Notes" }, { id: "audit", label: "Audit", disabled: true }]}>
      {#snippet panel(id)}
        {#if id === "invoices"}
          <div class="stack">
            <Callout variant="danger" title="2 invoices overdue">INV-2041 and INV-2047, 1 860 € in all, due 14 October.</Callout>
            <Callout variant="success" title="Paid on time">INV-2033 was paid on 2 October.</Callout>
          </div>
        {:else if id === "notes"}
          <Empty title="No notes yet" description="Notes are visible to everyone who can see this customer.">
            {#snippet action()}<Button size="sm" variant="primary">Add a note</Button>{/snippet}
          </Empty>
        {:else}
          <Skeleton lines={3} />
        {/if}
      {/snippet}
    </Tabs>
  </Card>

  <div class="block">
    <h3>The window's parts</h3>
    <div class="window">
      <Toolbar title="CRM">
        {#snippet actions()}<Spinner size="sm" label="Syncing" /><Button size="sm" variant="primary">New customer</Button>{/snippet}
      </Toolbar>
      <div class="window-middle">
        <Sidebar>
          <NavItem path="/">Overview</NavItem>
          <NavItem path="/customers" count={3}>Customers</NavItem>
          <NavItem path="/invoices">Invoices</NavItem>
          <Separator />
          <NavItem path="/settings">Settings</NavItem>
        </Sidebar>
        <div class="window-page"><Skeleton lines={4} /></div>
      </div>
      <StatusBar assistant={true}><span>4 customers</span></StatusBar>
    </div>
  </div>

  <Modal bind:open={modal} title="Delete customer" description="Ada Lovelace and her 12 invoices will be removed.">
    <p class="body">This can't be undone. Her invoices stay in the accounts.</p>
    {#snippet footer()}
      <Button onclick={() => (modal = false)}>Cancel</Button>
      <Button variant="danger" onclick={() => (modal = false)}>Delete customer</Button>
    {/snippet}
  </Modal>
  <Drawer bind:open={drawer} title="Filters">
    <div class="stack">
      <Field label="Country"><Select options={COUNTRIES} /></Field>
      <Switch>Only active</Switch>
    </div>
    {#snippet footer()}<Button variant="primary" onclick={() => (drawer = false)}>Apply</Button>{/snippet}
  </Drawer>
</section>

<style>
  .column {
    display: grid; gap: 18px; align-content: start; min-width: 0;
    padding: 20px; border-radius: 16px; border: 1px solid var(--elyra-border);
    background: var(--elyra-bg); color: var(--elyra-text);
    font: var(--elyra-font-size)/1.5 var(--elyra-font);
  }
  h2 { margin: 0; font-size: 12px; font-weight: 600; letter-spacing: .08em; text-transform: uppercase; color: var(--elyra-muted); }
  h3 { margin: 0 0 12px; font-size: 13px; font-weight: 600; }
  .block { padding: 16px; border-radius: 12px; background: var(--elyra-surface); border: 1px solid var(--elyra-border); }
  .row { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .row + .row { margin-top: 10px; }
  .row.end { justify-content: flex-end; }
  .two { display: grid; grid-template-columns: 1fr 1fr; gap: 12px; }
  .stack { display: grid; gap: 12px; }
  .note { color: var(--elyra-success); font-size: 13px; }
  .body { margin: 0; color: var(--elyra-muted); }
  .window { display: flex; flex-direction: column; height: 260px; overflow: hidden; border: 1px solid var(--elyra-border); border-radius: 10px; background: var(--elyra-bg); }
  .window-middle { display: flex; flex: 1; min-height: 0; }
  .window-middle :global(.elyra-sidebar) { width: 170px; }
  .window-page { flex: 1; padding: 16px; min-width: 0; }
  @media (max-width: 520px) { .two { grid-template-columns: 1fr; } }

  /* Each column carries its theme's tokens, whatever the page's theme. */
  .theme-dark {
    --elyra-bg: #16161e; --elyra-surface: #1a1b26; --elyra-raised: #1e1f2b; --elyra-inset: #1f2030;
    --elyra-text: #c0caf5; --elyra-muted: #787c99; --elyra-border: #2a2b3c; --elyra-focus: #7aa2f7;
    --elyra-accent: #7aa2f7; --elyra-accent-soft: #2f3650; --elyra-on-accent: #0b0b12;
    --elyra-danger: #f7768e; --elyra-success: #9ece6a; --elyra-warning: #e0af68;
    --elyra-shadow: 0 16px 40px rgba(0, 0, 0, .45); --elyra-overlay: rgba(0, 0, 0, .5);
    color-scheme: dark;
  }
  .theme-light {
    --elyra-bg: #ffffff; --elyra-surface: #f5f6fa; --elyra-raised: #ffffff; --elyra-inset: #eceef3;
    --elyra-text: #2a2e3a; --elyra-muted: #6b7080; --elyra-border: #d8dbe3; --elyra-focus: #3b6fd6;
    --elyra-accent: #3b6fd6; --elyra-accent-soft: #dbe4f7; --elyra-on-accent: #ffffff;
    --elyra-danger: #d6435b; --elyra-success: #3f9142; --elyra-warning: #a86f0c;
    --elyra-shadow: 0 16px 40px rgba(20, 24, 40, .16); --elyra-overlay: rgba(20, 24, 40, .35);
    color-scheme: light;
  }
</style>
