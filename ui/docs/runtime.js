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

// No shell to watch: online, nothing waiting, nothing moving.
export const connection = writable({ state: "online", since: null, pending: 0, failed: 0 });
export const outbox = writable([]);
export const transfers = writable([]);
// An app's assistant is behind its backend; the docs have none.
export const assistant = { source: () => Promise.reject(new Error("no assistant in the docs")) };
export const openAssistant = () => {};
