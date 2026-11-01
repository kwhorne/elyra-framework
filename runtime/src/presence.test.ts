/**
 * Presence (RFC 0015): `presence(channel)` joins on its first subscriber,
 * follows `elyra:presence` for that channel only, and leaves a moment after
 * its last one, so a quick unmount and mount doesn't leave and join again.
 */
import { afterEach, describe, expect, it } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import { drainPump, errorResponse, okResponse, stubPendingFetch, tick, type PendingCall } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { presence } = await import("./index.js");
type State = Parameters<Parameters<ReturnType<typeof presence>["subscribe"]>[0]>[0];

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  // Leaving waits a moment: let it go, so the event channel is let go of
  // too, then answer what's outstanding.
  for (const off of unsubscribes) off();
  await tick(350);
  await drainPump(pending, []);
  pending = [];
  unsubscribes = [];
});

const last = (seen: State[]): State | undefined => seen[seen.length - 1];

async function request(path: string): Promise<PendingCall> {
  for (let i = 0; i < 200; i++) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
  throw new Error(`no request to ${path}`);
}

const calls = (path: string) => pending.filter((p) => p.url.endsWith(path)).length;
const bodyOf = (call: PendingCall) => decode(new Uint8Array(call.init!.body as ArrayBuffer));

const state = (channel: string, joined: boolean, ids: string[]): State => ({
  channel,
  joined,
  members: ids.map((id) => ({ id, info: { name: `User ${id}` } })),
  me: "7",
});

/** The shell sends `elyra:presence` events. */
async function events(...states: State[]): Promise<void> {
  (await request("/__events")).resolve(
    new Response(encode(states.map((s) => ["elyra:presence", s])), { status: 200 }),
  );
}

function follow(channel: string): { seen: State[]; off: () => void } {
  const seen: State[] = [];
  const off = presence(channel).subscribe((s) => seen.push(s));
  return { seen, off };
}

describe("presence", () => {
  it("joins on the first subscriber, and follows its channel's members", async () => {
    pending = stubPendingFetch();
    const { seen, off } = follow("customers.42");
    unsubscribes.push(off);
    expect(seen[0]).toEqual({ channel: "customers.42", joined: false, members: [], me: null });

    const join = await request("/__presence/join");
    expect(bodyOf(join)).toEqual({ channel: "customers.42" });
    join.resolve(okResponse(state("customers.42", false, [])));

    // Another channel's members aren't this one's.
    await events(state("customers.7", true, ["9"]), state("customers.42", true, ["7", "9"]));
    await expect.poll(() => last(seen)?.joined).toBe(true);
    expect(last(seen)!.members.map((m: { id: string }) => m.id)).toEqual(["7", "9"]);
    expect(seen.some((s) => s.channel !== "customers.42")).toBe(false);
  });

  it("joins once for many subscribers, and leaves a moment after the last", async () => {
    pending = stubPendingFetch();
    const a = follow("customers.43");
    const b = follow("customers.43");
    (await request("/__presence/join")).resolve(okResponse(state("customers.43", true, ["7"])));
    await tick(20);
    expect(calls("/__presence/join")).toBe(0);
    expect(last(b.seen)!.joined).toBe(true);

    a.off();
    b.off();
    await tick(100);
    expect(calls("/__presence/leave")).toBe(0);
    const leave = await request("/__presence/leave");
    expect(bodyOf(leave)).toEqual({ channel: "customers.43" });
    leave.resolve(okResponse(true));
  });

  it("doesn't leave and join again for a quick unmount and mount", async () => {
    pending = stubPendingFetch();
    const first = follow("customers.44");
    (await request("/__presence/join")).resolve(okResponse(state("customers.44", true, ["7"])));
    first.off();
    await tick(50);
    const second = follow("customers.44");
    unsubscribes.push(second.off);
    expect(second.seen[0]!.joined).toBe(true);
    await tick(400);
    expect(calls("/__presence/leave")).toBe(0);
    expect(calls("/__presence/join")).toBe(0);
  });

  it("leaves a moment after the last unsubscribe, not the first", async () => {
    pending = stubPendingFetch();
    const first = follow("customers.48");
    (await request("/__presence/join")).resolve(okResponse(state("customers.48", true, ["7"])));
    first.off();
    await tick(50);
    const second = follow("customers.48");
    await tick(200);
    second.off();
    // 300 ms after the first unsubscribe, but only 100 after the last.
    await tick(150);
    expect(calls("/__presence/leave")).toBe(0);
    (await request("/__presence/leave")).resolve(okResponse(true));
  });

  it("isn't put back to not-joined by a join answer that comes after the members", async () => {
    pending = stubPendingFetch();
    const { seen, off } = follow("customers.45");
    unsubscribes.push(off);
    const join = await request("/__presence/join");
    await events(state("customers.45", true, ["7", "9"]));
    await expect.poll(() => last(seen)?.joined).toBe(true);
    join.resolve(okResponse(state("customers.45", false, [])));
    await tick(20);
    expect(last(seen)!.joined).toBe(true);
    expect(last(seen)!.members).toHaveLength(2);
  });

  it("ignores an event from before it subscribed, and stays empty where there's no presence", async () => {
    pending = stubPendingFetch();
    // An earlier subscription heard customers.46 with someone in it, and left.
    const before = follow("customers.47");
    (await request("/__presence/join")).resolve(okResponse(state("customers.47", false, [])));
    await events(state("customers.46", true, ["9"]));
    await tick(20);
    before.off();

    const { seen, off } = follow("customers.46");
    unsubscribes.push(off);
    (await request("/__presence/join")).resolve(errorResponse("this app joins no presence channels", "command"));
    await tick(20);
    expect(seen.every((s) => !s.joined && s.members.length === 0)).toBe(true);
  });
});
