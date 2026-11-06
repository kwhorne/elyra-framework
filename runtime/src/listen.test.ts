/**
 * Private channels a window listens to (RFC 0023): `listen(channel, handler)`
 * and the `listening(channel)` store listen on their first holder, share one
 * subscription however many hold it, hear only their channel's events on
 * `elyra:backend`, and stop a moment after the last holder lets go.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import { drainPump, errorResponse, okResponse, stubPendingFetch, tick, type PendingCall } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { listen, listening } = await import("./index.js");
type State = Parameters<Parameters<ReturnType<typeof listening>["subscribe"]>[0]>[0];
type BackendEvent = { channel: string; event: string; data: unknown };

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  // Stopping waits a moment: let it go, so the event channels are let go
  // of too, then answer what's outstanding.
  for (const off of unsubscribes) off();
  await tick(350);
  await drainPump(pending, []);
  pending = [];
  unsubscribes = [];
  vi.restoreAllMocks();
});

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
const last = <T>(seen: T[]): T | undefined => seen[seen.length - 1];

const ev = (channel: string, event: string, data: unknown): BackendEvent => ({ channel, event, data });

/** The shell sends a batch of events: `[channel, value]` pairs. */
async function events(...batch: Array<[string, unknown]>): Promise<void> {
  (await request("/__events")).resolve(new Response(encode(batch), { status: 200 }));
}

describe("listen", () => {
  it("listens on the first handler and hears only its channel's events", async () => {
    pending = stubPendingFetch();
    const heard: Array<[unknown, BackendEvent]> = [];
    unsubscribes.push(listen("task.1", (data, e) => heard.push([data, e])));

    const join = await request("/__listen/join");
    expect(bodyOf(join)).toEqual({ channel: "task.1" });
    join.resolve(okResponse({ channel: "task.1", joined: true }));

    await events(
      ["elyra:backend", ev("task.2", "RunOutput", { chunk: "not mine" })],
      ["elyra:backend", ev("task.1", "RunOutput", { chunk: "hello" })],
      ["elyra:backend", ev("task.1", "RunStatusChanged", { status: "done" })],
    );
    await expect.poll(() => heard.length).toBe(2);
    expect(heard[0]).toEqual([{ chunk: "hello" }, ev("task.1", "RunOutput", { chunk: "hello" })]);
    expect(heard[1]![1].event).toBe("RunStatusChanged");
  });

  it("narrows to one event when given its name", async () => {
    pending = stubPendingFetch();
    const outputs: unknown[] = [];
    unsubscribes.push(listen("task.3", "RunOutput", (data) => outputs.push(data)));
    (await request("/__listen/join")).resolve(okResponse({ channel: "task.3", joined: true }));
    await events(
      ["elyra:backend", ev("task.3", "RunStatusChanged", { status: "running" })],
      ["elyra:backend", ev("task.3", "RunOutput", { chunk: "a" })],
    );
    await expect.poll(() => outputs).toEqual([{ chunk: "a" }]);
  });

  it("subscribes once for many handlers and stores, and stops a moment after the last", async () => {
    pending = stubPendingFetch();
    const offA = listen("task.4", () => {});
    const offB = listen("task.4", () => {});
    const offStore = listening("task.4").subscribe(() => {});
    (await request("/__listen/join")).resolve(okResponse({ channel: "task.4", joined: true }));
    await tick(20);
    expect(calls("/__listen/join")).toBe(0);

    offA();
    offA();
    offB();
    await tick(400);
    expect(calls("/__listen/leave")).toBe(0);
    offStore();
    await tick(100);
    expect(calls("/__listen/leave")).toBe(0);
    const leave = await request("/__listen/leave");
    expect(bodyOf(leave)).toEqual({ channel: "task.4" });
    leave.resolve(okResponse(true));
  });

  it("doesn't stop and listen again for a quick unmount and mount", async () => {
    pending = stubPendingFetch();
    const first = listen("task.5", () => {});
    (await request("/__listen/join")).resolve(okResponse({ channel: "task.5", joined: true }));
    first();
    await tick(50);
    unsubscribes.push(listen("task.5", () => {}));
    await tick(400);
    expect(calls("/__listen/leave")).toBe(0);
    expect(calls("/__listen/join")).toBe(0);
  });

  it("keeps hearing in the other handlers when one throws", async () => {
    pending = stubPendingFetch();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const heard: unknown[] = [];
    unsubscribes.push(
      listen("task.6", () => {
        throw new Error("boom");
      }),
    );
    unsubscribes.push(listen("task.6", (data) => heard.push(data)));
    (await request("/__listen/join")).resolve(okResponse({ channel: "task.6", joined: true }));
    await events(["elyra:backend", ev("task.6", "RunOutput", 1)], ["elyra:backend", ev("task.6", "RunOutput", 2)]);
    await expect.poll(() => heard).toEqual([1, 2]);
    expect(error).toHaveBeenCalledTimes(2);
  });

  it("says why when the app refuses the channel, and hears nothing", async () => {
    pending = stubPendingFetch();
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    const heard: unknown[] = [];
    unsubscribes.push(listen("invoices.1", (data) => heard.push(data)));
    (await request("/__listen/join")).resolve(
      errorResponse("`invoices.1` isn't a private channel this app listens to (Backend::listen)", "denied"),
    );
    await expect.poll(() => error.mock.calls.length).toBe(1);
    expect(String(error.mock.calls[0]![0])).toContain("Backend::listen");
    expect(heard).toEqual([]);
  });
});

describe("listening", () => {
  it("holds whether it's joined, and the last event", async () => {
    pending = stubPendingFetch();
    const seen: State[] = [];
    unsubscribes.push(listening("task.7").subscribe((s) => seen.push(s)));
    expect(seen[0]).toEqual({ channel: "task.7", joined: false, last: null });

    const join = await request("/__listen/join");
    join.resolve(okResponse({ channel: "task.7", joined: false }));
    await events(["elyra:listening", { channel: "task.8", joined: true }], ["elyra:listening", { channel: "task.7", joined: true }]);
    await expect.poll(() => last(seen)?.joined).toBe(true);
    expect(seen.every((s) => s.channel === "task.7")).toBe(true);

    await events(["elyra:backend", ev("task.7", "RunOutput", { chunk: "x" })]);
    await expect.poll(() => last(seen)?.last?.event).toBe("RunOutput");

    // The connection drops: not joined, until it's back.
    await events(["elyra:listening", { channel: "task.7", joined: false }]);
    await expect.poll(() => last(seen)?.joined).toBe(false);
    expect(last(seen)!.last).toEqual(ev("task.7", "RunOutput", { chunk: "x" }));
  });

  it("isn't put back to not-joined by a join answer that comes after an event", async () => {
    pending = stubPendingFetch();
    const seen: State[] = [];
    unsubscribes.push(listening("task.9").subscribe((s) => seen.push(s)));
    const join = await request("/__listen/join");
    await events(["elyra:listening", { channel: "task.9", joined: true }]);
    await expect.poll(() => last(seen)?.joined).toBe(true);
    join.resolve(okResponse({ channel: "task.9", joined: false }));
    await tick(20);
    expect(last(seen)!.joined).toBe(true);
  });

  it("ignores what came before it subscribed", async () => {
    pending = stubPendingFetch();
    // An earlier holder heard task.10's event, and let go.
    const before = listen("task.11", () => {});
    (await request("/__listen/join")).resolve(okResponse({ channel: "task.11", joined: true }));
    await events(["elyra:backend", ev("task.10", "RunOutput", 1)], ["elyra:listening", { channel: "task.10", joined: true }]);
    await tick(20);
    before();

    const seen: State[] = [];
    const heard: unknown[] = [];
    unsubscribes.push(listening("task.10").subscribe((s) => seen.push(s)));
    unsubscribes.push(listen("task.10", (d) => heard.push(d)));
    (await request("/__listen/join")).resolve(okResponse({ channel: "task.10", joined: false }));
    await tick(20);
    expect(heard).toEqual([]);
    expect(seen.every((s) => !s.joined && s.last === null)).toBe(true);
  });
});
