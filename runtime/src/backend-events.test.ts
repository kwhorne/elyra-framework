/**
 * Broadcasting in the runtime (RFC 0006): `onBackendEvent` hands over the
 * events on a followed channel, `broadcasting` is the connection's state —
 * and no `on*` helper fires an old event again for a late subscriber.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { encode } from "@msgpack/msgpack";
import { drainPump, nthCall, okResponse, stubPendingFetch, tick, type PendingCall } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { onBackendEvent, onDeepLink, broadcasting, channel } = await import("./index.js");

function batch(pairs: [string, unknown][]): Response {
  return new Response(encode(pairs), { status: 200 });
}

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
  vi.unstubAllGlobals();
});

/** The next pending event poll (other requests are answered as they come). */
async function nextPoll(): Promise<PendingCall> {
  for (;;) {
    await tick();
    const poll = pending.find((p) => p.url.endsWith("/__events"));
    if (poll) {
      pending = pending.filter((p) => p !== poll);
      return poll;
    }
  }
}

describe("onBackendEvent", () => {
  it("hands over a followed channel's events, by name or all of them", async () => {
    pending = stubPendingFetch();
    const updated: unknown[] = [];
    const every: string[] = [];
    unsubscribes.push(onBackendEvent("customers", "CustomerUpdated", (data) => updated.push(data)));
    unsubscribes.push(onBackendEvent("customers", null, (_data, e) => every.push(e.event)));

    (await nthCall(pending, 1)).resolve(
      batch([
        ["elyra:backend", { channel: "customers", event: "CustomerCreated", data: { id: 1 } }],
        ["elyra:backend", { channel: "orders", event: "OrderShipped", data: {} }],
        ["elyra:backend", { channel: "customers", event: "CustomerUpdated", data: { id: 2 } }],
      ]),
    );
    await vi.waitFor(() => expect(every).toEqual(["CustomerCreated", "CustomerUpdated"]));
    expect(updated).toEqual([{ id: 2 }]);
  });
});

describe("on* helpers", () => {
  it("don't fire the last event again for a late subscriber", async () => {
    pending = stubPendingFetch();
    unsubscribes.push(channel("elyra:deep-link").subscribe(() => {}));
    (await nthCall(pending, 1)).resolve(batch([["elyra:deep-link", "myapp://open/1"]]));
    await tick();

    // The channel holds "myapp://open/1" now; a new listener must not get it.
    const late: string[] = [];
    unsubscribes.push(onDeepLink((url) => late.push(url)));
    expect(late).toEqual([]);

    (await nthCall(pending, 2)).resolve(batch([["elyra:deep-link", "myapp://open/2"]]));
    await vi.waitFor(() => expect(late).toEqual(["myapp://open/2"]));
  });
});

describe("broadcasting", () => {
  it("is the connection's state: asked once, then followed", async () => {
    pending = stubPendingFetch();
    const seen: string[] = [];
    unsubscribes.push(broadcasting.subscribe((s) => seen.push(s)));
    expect(seen).toEqual(["offline"]);

    const asked = pending.find((p) => p.url.endsWith("/__auth/broadcasting"));
    expect(asked).toBeDefined();
    asked!.resolve(okResponse({ state: "connected" }));
    await vi.waitFor(() => expect(seen[seen.length - 1]).toBe("connected"));

    (await nextPoll()).resolve(batch([["elyra:broadcasting", { state: "offline" }]]));
    await vi.waitFor(() => expect(seen[seen.length - 1]).toBe("offline"));
  });
});
