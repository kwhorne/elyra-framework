/**
 * The event bus: one multiplexed long-poll over `/__events`, fanned out to
 * per-channel subscribers.
 *
 * These tests drive the poll by hand (`stubPendingFetch`) rather than answering
 * immediately, because the behaviour worth pinning lives in the transitions —
 * when the pump reconnects, when it backs off, and when it gives up. A stub that
 * resolves on its own turns the pump into a busy loop and hides all of that.
 *
 * The subscriber map and the `pumping` flag live in module scope and vitest
 * shares that scope across the file, so every test registers its unsubscribes
 * with `sub()` and `drainPump` parks the pump again afterwards.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { encode } from "@msgpack/msgpack";
import {
  drainPump,
  headersOf,
  nthCall,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { channel } = await import("./index.js");

/** A batch response as the shell sends it: msgpack `[channel, value]` pairs. */
function batch(pairs: [string, unknown][]): Response {
  return new Response(encode(pairs), { status: 200 });
}

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

/** Subscribe, remembering the unsubscribe so the pump can be parked later. */
function sub<T>(name: string, handler: (value: T | undefined) => void): () => void {
  const off = channel<T>(name).subscribe(handler);
  unsubscribes.push(off);
  return off;
}

beforeEach(() => {
  pending = stubPendingFetch();
  unsubscribes = [];
});

afterEach(async () => {
  await drainPump(pending, unsubscribes);
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("channel — delivery", () => {
  it("delivers batched events in order and only to the matching channel", async () => {
    const ticks: number[] = [];
    const others: unknown[] = [];
    sub<number>("d.tick", (v) => v !== undefined && ticks.push(v));
    sub("d.other", (v) => v !== undefined && others.push(v));

    (await nthCall(pending, 1)).resolve(
      batch([
        ["d.tick", 1],
        ["d.tick", 2],
        ["d.other", "x"],
        ["d.unwatched", "ignored"],
      ]),
    );

    await vi.waitFor(() => expect(ticks).toEqual([1, 2]));
    expect(others).toEqual(["x"]);
  });

  it("multiplexes every channel over a single connection", async () => {
    sub("m.a", () => {});
    sub("m.b", () => {});
    sub("m.c", () => {});
    await tick();

    // Three channels, one poll — that is the whole point of the design.
    expect(pending).toHaveLength(1);
    expect(pending[0]!.url).toBe("elyra://localhost/__events");
  });

  it("fans one event out to every subscriber on the channel", async () => {
    const a: unknown[] = [];
    const b: unknown[] = [];
    sub("f.tick", (v) => v !== undefined && a.push(v));
    sub("f.tick", (v) => v !== undefined && b.push(v));

    (await nthCall(pending, 1)).resolve(batch([["f.tick", 7]]));

    await vi.waitFor(() => {
      expect(a).toEqual([7]);
      expect(b).toEqual([7]);
    });
  });

  it("emits undefined immediately for a channel that has never fired", () => {
    const seen: unknown[] = [];
    sub("r.fresh", (v) => seen.push(v));

    // Svelte store contract: subscribing always calls the handler synchronously.
    expect(seen).toEqual([undefined]);
  });

  it("replays the last value to a late subscriber", async () => {
    sub<number>("r.tick", () => {});
    (await nthCall(pending, 1)).resolve(
      batch([
        ["r.tick", 1],
        ["r.tick", 2],
      ]),
    );
    await tick();

    const replay: (number | undefined)[] = [];
    sub<number>("r.tick", (v) => replay.push(v));
    expect(replay[0]).toBe(2);
  });

  it("stops delivering after unsubscribe", async () => {
    const seen: unknown[] = [];
    const off = sub("u.tick", (v) => v !== undefined && seen.push(v));
    // A second subscriber keeps the pump alive, so this isolates unsubscribe
    // from the shutdown path.
    sub("u.keepalive", () => {});

    (await nthCall(pending, 1)).resolve(batch([["u.tick", 1]]));
    await vi.waitFor(() => expect(seen).toEqual([1]));

    off();

    (await nthCall(pending, 2)).resolve(batch([["u.tick", 2]]));
    await tick(10);
    expect(seen).toEqual([1]);
  });

  it("keeps pumping when a subscriber throws", async () => {
    // dispatch() runs inside the pump's try block, so a throwing handler is
    // indistinguishable from a transport failure: it costs a backoff, but the
    // bus must recover rather than die.
    const good: unknown[] = [];
    sub("t.tick", (v) => {
      if (v !== undefined) throw new Error("handler blew up");
    });
    sub("t.other", (v) => v !== undefined && good.push(v));

    (await nthCall(pending, 1)).resolve(batch([["t.tick", 1]]));

    (await nthCall(pending, 2)).resolve(batch([["t.other", "still here"]]));
    await vi.waitFor(() => expect(good).toEqual(["still here"]));
  });
});

describe("channel — the poll request", () => {
  it("authenticates the long-poll and asks for msgpack", async () => {
    sub("h.tick", () => {});
    const call = await nthCall(pending, 1);

    const headers = headersOf(call);
    expect(headers.get("x-elyra-token")).toBe("test-token");
    expect(headers.get("x-elyra-client-id")).toMatch(/.+/);
    expect(headers.get("accept")).toBe("application/msgpack");
  });

  it("reconnects after each batch, and starts a fresh pump after going idle", async () => {
    const off = sub("l.tick", () => {});
    (await nthCall(pending, 1)).resolve(batch([["l.tick", 1]]));

    // A served batch is followed by a new poll: the connection is a loop.
    await nthCall(pending, 2);

    // Drop the last subscriber, then answer the in-flight poll. With nobody
    // listening the loop must exit instead of polling again.
    off();
    pending[1]!.resolve(batch([]));
    await tick(20);
    expect(pending).toHaveLength(2);

    // ...and it must have cleared `pumping` on the way out, or the bus would be
    // permanently dead for the next subscriber.
    sub("l.tick", () => {});
    await nthCall(pending, 3);
  });
});

describe("channel — failure handling", () => {
  it("retries after a transport failure", async () => {
    const seen: unknown[] = [];
    sub("e.tick", (v) => v !== undefined && seen.push(v));

    (await nthCall(pending, 1)).reject(new Error("socket closed"));

    // Backoff starts at 100ms: quick, but not a hot loop.
    (await nthCall(pending, 2)).resolve(batch([["e.tick", "recovered"]]));
    await vi.waitFor(() => expect(seen).toEqual(["recovered"]));
  });

  it("retries a non-ok poll response", async () => {
    sub("s.tick", () => {});
    (await nthCall(pending, 1)).resolve(new Response("nope", { status: 500 }));

    await nthCall(pending, 2);
  });

  it("gives up permanently when the token is rejected", async () => {
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});

    sub("x.tick", () => {});
    (await nthCall(pending, 1)).resolve(
      new Response("missing or invalid x-elyra-token", {
        status: 403,
        headers: { "x-elyra-status": "error", "x-elyra-error-kind": "forbidden" },
      }),
    );

    // A rejected token will never start working, so retrying is pure noise. The
    // pump must break out and say why exactly once.
    await vi.waitFor(() => expect(logged).toHaveBeenCalledTimes(1));
    expect(String(logged.mock.calls[0]![0])).toContain("IPC token");

    await tick(300);
    expect(pending).toHaveLength(1);
  });
});
