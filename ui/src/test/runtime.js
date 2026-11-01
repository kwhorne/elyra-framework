// What the components use from @elyra/runtime, for the tests: the user's
// abilities, the confirm dialog and the router stand-ins; the real
// validationErrors.
import { vi } from "vitest";

let granted = ["*"];
const listeners = new Set();
const covers = (g, a) => (g.endsWith("*") ? a.startsWith(g.slice(0, -1)) : g === a);

/** Set what the signed-in user may do. */
export function grant(abilities) {
  granted = abilities;
  for (const run of listeners) run((a) => granted.some((g) => covers(g, a)));
}

export const can = {
  subscribe(run) {
    listeners.add(run);
    run((a) => granted.some((g) => covers(g, a)));
    return () => listeners.delete(run);
  },
};

export const confirm = vi.fn(async () => true);
export const href = (path) => `#${path}`;

// The stores the layout components follow, set by the tests.
import { writable } from "svelte/store";

export const route = writable({ path: "/", query: {} });
export const connection = writable({ state: "online", since: null, pending: 0, failed: 0 });
const outboxItems = writable([]);
const drop = (id) => outboxItems.update((items) => items.filter((w) => w.id !== id));
/** The outbox: set by the tests; each way out takes the write off the list. */
export const outbox = {
  subscribe: outboxItems.subscribe,
  set: outboxItems.set,
  retry: vi.fn(async (id) => drop(id)),
  discard: vi.fn(async (id) => drop(id)),
  keepMine: vi.fn(async (id) => drop(id)),
  takeTheirs: vi.fn(async (id) => drop(id)),
};
export const transfers = writable([]);
export const openAssistant = vi.fn();
/** Whether the app has an assistant: its `source()` answers, or refuses. */
export const assistant = { source: vi.fn(async () => "backend"), selected: vi.fn(async () => {}) };

/** The file dialog: answers with what the test sets (nothing, by default). */
export const dialog = { open: vi.fn(async () => []) };
const dropHandlers = new Set();
export function onFileDrop(handler) {
  dropHandlers.add(handler);
  return () => dropHandlers.delete(handler);
}
/** Drop files on the window. */
export function dropFiles(files) {
  for (const handler of dropHandlers) handler(files);
}
/** How many are listening for drops. */
export const dropListeners = () => dropHandlers.size;

/** The user's abilities, as the shell serves them (`masked` from the inspector). */
export const abilities = writable({ loaded: true, abilities: ["*"] });
/** The inspector (RFC 0014): there or not, as the test says. */
export const inspector = { available: vi.fn(async () => false) };
export const openInspector = vi.fn();

/** Presence (RFC 0015): a store per channel, and what the shell was asked. */
const channels = new Map();
export const presenceJoined = [];
export const presence = vi.fn((channel) => {
  if (!channels.has(channel)) channels.set(channel, writable({ channel, joined: false, members: [], me: null }));
  const store = channels.get(channel);
  return {
    subscribe(run) {
      presenceJoined.push(channel);
      return store.subscribe(run);
    },
  };
});
/** The shell says who's in `channel`. */
export function setPresence(channel, state) {
  presence(channel);
  channels.get(channel).set({ channel, ...state });
}

/** The settings store: an object in memory. */
const settings = new Map();
export const store = {
  get: vi.fn(async (key) => (settings.has(key) ? settings.get(key) : null)),
  set: vi.fn(async (key, value) => void settings.set(key, value)),
};

/** Back to an app at `/`, online, with nothing waiting. */
export function resetStores() {
  route.set({ path: "/", query: {} });
  connection.set({ state: "online", since: null, pending: 0, failed: 0 });
  outbox.set([]);
  for (const way of ["retry", "discard", "keepMine", "takeTheirs"]) {
    outbox[way].mockReset();
    outbox[way].mockImplementation(async (id) => drop(id));
  }
  transfers.set([]);
  dialog.open.mockReset();
  dialog.open.mockResolvedValue([]);
  settings.clear();
  store.get.mockClear();
  store.set.mockClear();
  abilities.set({ loaded: true, abilities: ["*"] });
  channels.clear();
  presenceJoined.length = 0;
  inspector.available.mockReset();
  inspector.available.mockResolvedValue(false);
  openInspector.mockReset();
  openAssistant.mockReset();
  assistant.selected.mockReset();
  assistant.selected.mockResolvedValue(undefined);
  assistant.source.mockReset();
  assistant.source.mockResolvedValue("backend");
}
