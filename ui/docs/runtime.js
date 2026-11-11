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
export const assistant = {
  source: () => Promise.reject(new Error("no assistant in the docs")),
  selected: async () => {},
};
export const openAssistant = () => {};

// No native dialog in a browser: the file dialog "chooses" a sample receipt,
// and no file is ever dropped on the window.
export const dialog = {
  open: async () => [{ id: "docs", path: "/Users/ada/Documents/receipt-2041.pdf", name: "receipt-2041.pdf", kind: "open" }],
  save: async () => null,
};
export const onFileDrop = () => () => {};

// The settings store is the shell's; here it's the browser's localStorage, so
// a SplitPane's `persist` keeps its size across reloads of the docs.
const docsKey = (key) => `elyra-docs:${key}`;
export const store = {
  get: async (key) => JSON.parse(localStorage.getItem(docsKey(key)) ?? "null"),
  set: async (key, value) => localStorage.setItem(docsKey(key), JSON.stringify(value)),
  delete: async (key) => (localStorage.removeItem(docsKey(key)), true),
  all: async () => ({}),
  clear: async () => {},
};

// No broadcasting: Ada and Linus have the customer open on the web, Ada is
// editing it, and the reader is Grete.
export const presence = (channel) => ({
  subscribe(run) {
    run({
      channel,
      joined: true,
      me: "7",
      members: [
        { id: "7", info: { name: "Grete Waitz" }, editing: false },
        { id: "9", info: { name: "Ada Lovelace" }, editing: true },
        { id: "12", info: { name: "Linus Torvalds" }, editing: false },
      ],
    });
    return () => {};
  },
  editing() {},
});

// Opening a page in the app moves the example's route, not the docs'.
export const navigate = (path) => route.set({ path, params: {}, query: {} });

// Laravel's notifications: two unread from this morning, one read last week.
// Reading one marks it here, until the page reloads.
const minutesAgo = (n) => new Date(Date.now() - n * 60_000).toISOString();
const notificationsState = writable({
  unread: 2,
  hasMore: false,
  items: [
    { id: "n1", kind: "InvoicePaid", readAt: null, createdAt: minutesAgo(4), data: { title: "Invoice 2041 paid", body: "Ada Lovelace AS paid 12 500 kr", url: "/invoices/2041" } },
    { id: "n2", kind: "Mentioned", readAt: null, createdAt: minutesAgo(95), data: { title: "Linus mentioned you", body: "“Can you look at Kunde AS's credit limit?”", url: "/customers/7" } },
    { id: "n3", kind: "WeeklySummary", readAt: minutesAgo(9000), createdAt: minutesAgo(9000), data: { title: "Your week", body: "14 invoices sent, 11 paid" } },
  ],
});
const markRead = (ids) =>
  notificationsState.update((s) => {
    const items = s.items.map((n) => (ids.includes(n.id) && !n.readAt ? { ...n, readAt: new Date().toISOString() } : n));
    return { ...s, items, unread: items.filter((n) => !n.readAt).length };
  });
export const notifications = {
  subscribe: notificationsState.subscribe,
  refresh: async () => {},
  more: async () => {},
  markRead: async (id) => markRead([id]),
  markAllRead: async () => {
    let all = [];
    notificationsState.subscribe((s) => (all = s.items.map((n) => n.id)))();
    markRead(all);
  },
};

// Signed in to two servers: Ada's employer, Acme, and a customer, Kunde.
// Switching moves the check; signing out takes a server off, until the page reloads.
const accountsState = writable({
  accounts: [
    { server: "https://acme.example.com", userId: "7", user: { name: "Ada Lovelace", email: "ada@acme.example.com" }, active: true },
    { server: "https://kunde.example.com", userId: "41", user: { name: "Ada Lovelace", email: "ada@kunde.example.com" }, active: false, pending: 3, failed: 1 },
  ],
  active: "https://acme.example.com",
  signedOutOf: null,
  reason: null,
});
export const accounts = { subscribe: accountsState.subscribe };
const activate = (list, server) => list.map((a) => ({ ...a, active: a.server === server }));
const leaveServer = (server) =>
  accountsState.update((s) => {
    const rest = s.accounts.filter((a) => a.server !== server);
    const active = s.active === server ? (rest[0]?.server ?? null) : s.active;
    return { ...s, accounts: activate(rest, active), active, signedOutOf: server, reason: "signed-out" };
  });
export const auth = {
  switchTo: async (server) => accountsState.update((s) => ({ ...s, accounts: activate(s.accounts, server), active: server })),
  signOutOf: async (server) => leaveServer(server),
  signOutOfDiscarding: async (server) => leaveServer(server),
  signOutAll: async () => accountsState.update((s) => ({ ...s, accounts: [], active: null })),
  signOutAllDiscarding: async () => accountsState.update((s) => ({ ...s, accounts: [], active: null })),
  subscribe(run) {
    run({ signedIn: true, user: { name: "Ada Lovelace" }, reason: null });
    return () => {};
  },
};

// No microphone in a browser: a recording "hears" a level that rises and
// falls, and is transcribed to a sample sentence.
const micHandlers = new Set();
let micTimer = null;
const SAMPLE = { id: "sample", path: "/recordings/recording-1.wav", name: "recording-1.wav", kind: "open" };
export const microphone = {
  async start() {
    let t = 0;
    micTimer = setInterval(() => {
      t += 1;
      const level = 0.25 + 0.45 * Math.abs(Math.sin(t / 3));
      for (const handler of micHandlers) handler({ type: "level", level });
    }, 66);
  },
  async stop() {
    clearInterval(micTimer);
    return SAMPLE;
  },
  async cancel() {
    clearInterval(micTimer);
  },
  async transcribe() {
    await new Promise((resolve) => setTimeout(resolve, 600));
    return "Send a reminder on these, and set the new due date to Friday.";
  },
};
export const onMicrophone = (handler) => {
  micHandlers.add(handler);
  return () => micHandlers.delete(handler);
};
