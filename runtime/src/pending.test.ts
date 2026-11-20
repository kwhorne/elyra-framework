/**
 * Shown at once (RFC 0039): `$pending` says which records have a write not
 * yet confirmed by the server.
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

const { pending } = await import("./index.js");
type Pending = import("./index.js").Pending;

let calls: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(calls, unsubscribes);
});

async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = calls.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return calls.splice(at, 1)[0]!;
  }
}

describe("pending", () => {
  it("is read once, then followed as writes start and are confirmed", async () => {
    calls = stubPendingFetch();
    let seen: Pending = {};
    unsubscribes.push(pending.subscribe((p) => (seen = p)));
    expect(seen).toEqual({});

    (await request("/__auth/pending")).resolve(okResponse({ customers: [42] }));
    await vi.waitFor(() => expect(seen.customers).toEqual([42]));

    (await request("/__events")).resolve(
      new Response(encode([["elyra:pending", { customers: [42], "customers/42/notes": [7] }]]), {
        status: 200,
      }),
    );
    await vi.waitFor(() => expect(seen["customers/42/notes"]).toEqual([7]));

    (await request("/__events")).resolve(
      new Response(encode([["elyra:pending", {}]]), { status: 200 }),
    );
    await vi.waitFor(() => expect(seen).toEqual({}));
  });
});
