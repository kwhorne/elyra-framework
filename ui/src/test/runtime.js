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
export const outbox = writable([]);
export const transfers = writable([]);
export const openAssistant = vi.fn();
/** Whether the app has an assistant: its `source()` answers, or refuses. */
export const assistant = { source: vi.fn(async () => "backend") };

/** Back to an app at `/`, online, with nothing waiting. */
export function resetStores() {
  route.set({ path: "/", query: {} });
  connection.set({ state: "online", since: null, pending: 0, failed: 0 });
  outbox.set([]);
  transfers.set([]);
  openAssistant.mockReset();
  assistant.source.mockReset();
  assistant.source.mockResolvedValue("backend");
}
