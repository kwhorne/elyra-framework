/**
 * What the signed-in user may do (RFC 0010): the `abilities` store, read
 * once then followed, and `$can(..)` over it, namespaces included.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { encode } from "@msgpack/msgpack";
import {
  drainPump,
  okResponse,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { abilities, can } = await import("./index.js");

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
});

async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

describe("can", () => {
  it("answers no until the list is read, then follows it", async () => {
    pending = stubPendingFetch();
    let check: (ability: string) => boolean = () => true;
    unsubscribes.push(can.subscribe((c) => (check = c)));
    expect(check("customers.view")).toBe(false);

    (await request("/__auth/abilities")).resolve(
      okResponse({ loaded: true, abilities: ["customers.view", "invoices.*"] }),
    );
    await vi.waitFor(() => expect(check("customers.view")).toBe(true));
    expect(check("customers.delete")).toBe(false);
    expect(check("invoices.delete")).toBe(true);
    expect(check("invoicesX")).toBe(false);

    // Changed on the web: Rust publishes the new list.
    (await request("/__events")).resolve(
      new Response(
        encode([["elyra:abilities", { loaded: true, abilities: ["customers.*"] }]]),
        { status: 200 },
      ),
    );
    await vi.waitFor(() => expect(check("customers.delete")).toBe(true));
    expect(check("invoices.delete")).toBe(false);
  });

  it("an app without Laravel's abilities may do what it grants: *", async () => {
    pending = stubPendingFetch();
    let check: (ability: string) => boolean = () => false;
    unsubscribes.push(can.subscribe((c) => (check = c)));
    (await request("/__auth/abilities")).resolve(okResponse({ loaded: true, abilities: ["*"] }));
    await vi.waitFor(() => expect(check("anything.at.all")).toBe(true));
  });
});

describe("abilities", () => {
  it("is the list, and reads it again on request", async () => {
    pending = stubPendingFetch();
    let seen = { loaded: false, abilities: [] as string[] };
    unsubscribes.push(abilities.subscribe((s) => (seen = s)));
    (await request("/__auth/abilities")).resolve(okResponse({ loaded: true, abilities: ["a"] }));
    await vi.waitFor(() => expect(seen.abilities).toEqual(["a"]));

    const done = abilities.refresh();
    (await request("/__auth/abilities-refresh")).resolve(
      okResponse({ loaded: true, abilities: ["a", "b"] }),
    );
    await done;
    expect(seen.abilities).toEqual(["a", "b"]);
  });
});
