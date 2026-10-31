// The reference runs in a browser, without the Elyra shell: the real
// @elyra/runtime, with what needs the app stood in for.
export * from "../../runtime/dist/index.js";
import { writable } from "svelte/store";

/** The reader's abilities: everything but deleting customers, so `can` shows. */
export const GRANTED = ["customers.view", "customers.create", "customers.update", "customers.export"];

export const can = {
  subscribe(run) {
    run((ability) => GRANTED.includes(ability));
    return () => {};
  },
};

// Links into the app (`href="/customers"`, a NavItem) stay in the example:
// they change this route, not the page (see App.svelte).
export const route = writable({ path: "/customers", params: {}, query: {} });
export const href = (path) => `#!${path}`;

// No shell to watch: online, nothing moving. The outbox has three writes
// from a morning offline, so the StatusBar and the OutboxDrawer have
// something to show; each way out takes one off, until the page reloads.
export const connection = writable({ state: "online", since: null, pending: 1, failed: 2 });
const saved = new Date();
saved.setHours(9, 42, 0, 0);
const writes = writable([
  {
    id: 1, method: "PUT", path: "/api/customers/42", status: "failed", attempts: 1,
    body: { name: "Ada Lovelace", credit_limit: 50000 },
    error: "Someone changed this customer meanwhile.", errors: null, reason: "conflict", createdAt: +saved,
  },
  {
    id: 2, method: "POST", path: "/api/customers", status: "failed", attempts: 1,
    body: { name: "Grace Hopper", email: "ada@example.com", contract: { file: "contract.pdf" } },
    error: "The email has already been taken.", errors: { email: ["The email has already been taken."] },
    reason: "validation", createdAt: +saved + 60_000,
  },
  {
    id: 3, method: "DELETE", path: "/api/invoices/2041", status: "pending", attempts: 3,
    body: null, error: null, errors: null, reason: null, createdAt: +saved + 120_000,
  },
]);
const drop = async (id) => writes.update((items) => items.filter((w) => w.id !== id));
export const outbox = { subscribe: writes.subscribe, retry: drop, discard: drop, keepMine: drop, takeTheirs: drop };
export const transfers = writable([]);
// An app's assistant is behind its backend; the docs have none.
export const assistant = { source: () => Promise.reject(new Error("no assistant in the docs")) };
export const openAssistant = () => {};
