/**
 * The ⌘K palette's searches: `registerSearch` and `search(query)`, which
 * asks the sources the user may use and is typing enough for, in the order
 * they were registered, a failing one left out.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  drainPump,
  okResponse,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { can, registerSearch, search } = await import("./index.js");
type Source = Parameters<typeof registerSearch>[0];

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];
let registered: Array<() => void> = [];

afterEach(async () => {
  for (const off of registered) off();
  registered = [];
  await drainPump(pending, unsubscribes);
  pending = [];
  unsubscribes = [];
});

async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

/** The user may do `granted`: read through `can`, as the palette does. */
async function grant(granted: string[]): Promise<void> {
  pending = stubPendingFetch();
  let check: (ability: string) => boolean = () => false;
  unsubscribes.push(can.subscribe((c) => (check = c)));
  (await request("/__auth/abilities")).resolve(okResponse({ loaded: true, abilities: granted }));
  // The list from the test before may still be there: wait for this one.
  const everything = granted.includes("*");
  await vi.waitFor(() => {
    expect(granted.every((g) => check(g.replace("*", "x")))).toBe(true);
    expect(check("not.granted")).toBe(everything);
  });
}

function source(id: string, names: string[], extra: Partial<Source> = {}): Source & { asked: string[] } {
  const asked: string[] = [];
  return {
    id,
    title: id[0]!.toUpperCase() + id.slice(1),
    asked,
    async search(query) {
      asked.push(query);
      return names
        .filter((n) => n.toLowerCase().includes(query.toLowerCase()))
        .map((n) => ({ title: n, action: () => {} }));
    },
    ...extra,
  };
}

const add = (s: Source) => registered.push(registerSearch(s));
const titles = (groups: Awaited<ReturnType<typeof search>>) =>
  groups.map((g) => [g.title, g.results.map((r) => r.title)]);

describe("search", () => {
  it("asks each source, in the order registered, and keeps those that found something", async () => {
    await grant(["*"]);
    add(source("customers", ["Ada Lovelace", "Grace Hopper"]));
    add(source("invoices", ["Invoice 7"]));
    add(source("teams", ["Analytical Engines"]));
    expect(titles(await search("  ad "))).toEqual([["Customers", ["Ada Lovelace"]]]);
    expect(titles(await search("an"))).toEqual([["Teams", ["Analytical Engines"]]]);
    expect(titles(await search("in"))).toEqual([["Invoices", ["Invoice 7"]], ["Teams", ["Analytical Engines"]]]);
  });

  it("waits for enough characters: two, or the source's own", async () => {
    await grant(["*"]);
    const customers = source("customers", ["Ada"]);
    const codes = source("codes", ["A1"], { minChars: 1 });
    add(customers);
    add(codes);
    expect(titles(await search("a"))).toEqual([["Codes", ["A1"]]]);
    expect(customers.asked).toEqual([]);
    await search("ad");
    expect(customers.asked).toEqual(["ad"]);
  });

  it("doesn't ask a source the user may not use", async () => {
    await grant(["invoices.*"]);
    const customers = source("customers", ["Ada"], { can: "customers.view" });
    const invoices = source("invoices", ["Ada's invoice"], { can: "invoices.view" });
    add(customers);
    add(invoices);
    expect(titles(await search("ada"))).toEqual([["Invoices", ["Ada's invoice"]]]);
    expect(customers.asked).toEqual([]);
  });

  it("leaves out a source that fails, or answers with something that isn't a list", async () => {
    await grant(["*"]);
    add({ id: "broken", title: "Broken", search: () => Promise.reject(new Error("offline")) });
    add({ id: "odd", title: "Odd", search: () => Promise.resolve(null as never) });
    add(source("customers", ["Ada"]));
    expect(titles(await search("ada"))).toEqual([["Customers", ["Ada"]]]);
  });

  it("hands each source the signal", async () => {
    await grant(["*"]);
    const signals: AbortSignal[] = [];
    add({ id: "slow", title: "Slow", search: async (_q, { signal }) => (signals.push(signal), []) });
    const controller = new AbortController();
    await search("ada", { signal: controller.signal });
    expect(signals[0]).toBe(controller.signal);
  });

  it("replaces a source by its id, and removing a replaced one keeps the new one", async () => {
    await grant(["*"]);
    const off = registerSearch(source("customers", ["Ada"]));
    registered.push(registerSearch(source("customers", ["Adam"])));
    expect(titles(await search("ada"))).toEqual([["Customers", ["Adam"]]]);
    off();
    expect(titles(await search("ada"))).toEqual([["Customers", ["Adam"]]]);
    registered.pop()!();
    expect(await search("ada")).toEqual([]);
  });
});
