/**
 * Capabilities per window (RFC 0016): `$can` in a window with limits says
 * no to what they leave out, and keeps them when a change of the user's
 * list arrives (sent to every window, without the window's limits). In a
 * file of its own: the limits are this window's for the module's life.
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

describe("can, in a window with limits", () => {
  it("says no to what the window leaves out, and keeps its limits across changes", async () => {
    pending = stubPendingFetch();
    let check: (ability: string) => boolean = () => true;
    unsubscribes.push(can.subscribe((c) => (check = c)));
    (await request("/__auth/abilities")).resolve(
      okResponse({
        loaded: true,
        abilities: ["*"],
        window: { only: ["invoices.*", "customers.view"], deny: ["invoices.delete"] },
      }),
    );
    await vi.waitFor(() => expect(check("invoices.view")).toBe(true));
    expect(check("customers.view")).toBe(true);
    // Not in `only`.
    expect(check("customers.delete")).toBe(false);
    // In `only`, but denied.
    expect(check("invoices.delete")).toBe(false);

    // The user's list changes: the event carries no window, the limits stay.
    (await request("/__events")).resolve(
      new Response(encode([["elyra:abilities", { loaded: true, abilities: ["invoices.view"] }]]), {
        status: 200,
      }),
    );
    await vi.waitFor(() => expect(check("invoices.send")).toBe(false));
    expect(check("invoices.view")).toBe(true);
    expect(check("customers.view")).toBe(false);
    let seen: unknown;
    unsubscribes.push(abilities.subscribe((s) => (seen = s.window)));
    expect(seen).toEqual({ only: ["invoices.*", "customers.view"], deny: ["invoices.delete"] });
  });

  it("deny alone leaves the rest of the list", async () => {
    pending = stubPendingFetch();
    let check: (ability: string) => boolean = () => false;
    unsubscribes.push(can.subscribe((c) => (check = c)));
    (await request("/__auth/abilities")).resolve(
      okResponse({ loaded: true, abilities: ["*"], window: { deny: ["billing.*"] } }),
    );
    await vi.waitFor(() => expect(check("customers.delete")).toBe(true));
    expect(check("billing.refund")).toBe(false);
  });
});
