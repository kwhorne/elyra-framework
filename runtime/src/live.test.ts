/**
 * `live()`: a store over a `#[command(live)]` subscription — opened by the
 * first listener, fed by `elyra:live:<id>` on the event poll, closed by the
 * last listener. Every request stays pending until the test answers it, so
 * the order of what goes over the wire is what's asserted.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import {
  drainPump,
  errorResponse,
  headersOf,
  okResponse,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { live, CommandError, ValidationError } = await import("./index.js");
type Live<T> = import("./index.js").Live<T>;

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

/** The first unanswered request to `path`, once it's been made. */
async function request(path: string): Promise<PendingCall> {
  let found: PendingCall | undefined;
  await vi.waitFor(() => {
    found = pending.find((c) => c.url.endsWith(path) && !answered.has(c));
    expect(found).toBeDefined();
  });
  answered.add(found!);
  return found!;
}
const answered = new Set<PendingCall>();

function watch<T>(command: string, ...args: unknown[]): Live<T>[] {
  const states: Live<T>[] = [];
  unsubscribes.push(live<T>(command, ...args).subscribe((s) => states.push(s)));
  return states;
}

/** The latest state a store reported. */
const last = <T>(states: T[]): T | undefined => states[states.length - 1];

const batch = (pairs: [string, unknown][]) => new Response(encode(pairs), { status: 200 });

beforeEach(() => {
  pending = stubPendingFetch();
  unsubscribes = [];
  answered.clear();
});

afterEach(async () => {
  await drainPump(pending, unsubscribes);
  vi.unstubAllGlobals();
});

describe("live — opening", () => {
  it("subscribes on the first listener, with the arguments, the token and the window", async () => {
    const states = watch<number>("teams_count", { search: "a" }, 2);
    expect(states).toEqual([{ value: undefined, error: null, loading: true }]);

    const open = await request("/__live/teams_count");
    expect(open.init?.method).toBe("POST");
    expect(decode(open.init?.body as Uint8Array)).toEqual([{ search: "a" }, 2]);
    const headers = headersOf(open);
    expect(headers.get("x-elyra-token")).toBe("test-token");
    expect(headers.get("x-elyra-client-id")).toBeTruthy();

    open.resolve(okResponse({ id: "s1", value: 3 }));
    await vi.waitFor(() => expect(last(states)).toEqual({ value: 3, error: null, loading: false }));
  });

  it("opens once however many listeners there are", async () => {
    const store = live<number>("teams_count");
    unsubscribes.push(store.subscribe(() => {}), store.subscribe(() => {}));
    await tick();
    expect(pending.filter((c) => c.url.includes("/__live/"))).toHaveLength(1);
  });

  it("a refused subscription is an error, typed like a command's", async () => {
    const states = watch("customers_index");
    (await request("/__live/customers_index")).resolve(
      errorResponse('{"search":["The search must be a string."]}', "validation"),
    );
    await vi.waitFor(() => expect(last(states)?.loading).toBe(false));
    const error = last(states)!.error;
    expect(error).toBeInstanceOf(ValidationError);
    expect((error as InstanceType<typeof ValidationError>).errors.search).toEqual([
      "The search must be a string.",
    ]);
  });
});

describe("live — updates", () => {
  it("takes each pushed result, and keeps the value when a re-run fails", async () => {
    const states = watch<number>("teams_count");
    (await request("/__live/teams_count")).resolve(okResponse({ id: "s2", value: 1 }));

    (await request("/__events")).resolve(batch([["elyra:live:s2", { value: 2 }]]));
    await vi.waitFor(() => expect(last(states)?.value).toBe(2));

    (await request("/__events")).resolve(
      batch([["elyra:live:s2", { error: { message: "too many teams", kind: "command" } }]]),
    );
    await vi.waitFor(() => expect(last(states)?.error).toBeInstanceOf(CommandError));
    expect(last(states)?.value).toBe(2);
    expect(last(states)?.error?.message).toContain("too many teams");

    // A later success clears the error.
    (await request("/__events")).resolve(batch([["elyra:live:s2", { value: 3 }]]));
    await vi.waitFor(() => expect(last(states)).toEqual({ value: 3, error: null, loading: false }));
  });

  it("ignores another subscription's updates", async () => {
    const states = watch<number>("teams_count");
    (await request("/__live/teams_count")).resolve(okResponse({ id: "s3", value: 1 }));
    (await request("/__events")).resolve(batch([["elyra:live:other", { value: 99 }]]));
    await tick(20);
    expect(last(states)?.value).toBe(1);
  });
});

describe("live — closing", () => {
  it("the last listener leaving stops the subscription", async () => {
    const store = live<number>("teams_count");
    const a = store.subscribe(() => {});
    const b = store.subscribe(() => {});
    (await request("/__live/teams_count")).resolve(okResponse({ id: "s4", value: 1 }));
    await tick();

    a();
    await tick();
    expect(pending.some((c) => c.url.endsWith("/__live-stop"))).toBe(false);

    b();
    const stop = await request("/__live-stop");
    expect(decode(stop.init?.body as Uint8Array)).toBe("s4");
    stop.resolve(okResponse(true));
  });

  it("leaving while it opens stops it as soon as it's open", async () => {
    const off = live<number>("teams_count").subscribe(() => {});
    const open = await request("/__live/teams_count");
    off();
    open.resolve(okResponse({ id: "s5", value: 1 }));
    const stop = await request("/__live-stop");
    expect(decode(stop.init?.body as Uint8Array)).toBe("s5");
    stop.resolve(okResponse(true));
  });

  it("leaving and coming back while it opens doesn't leak the first one", async () => {
    const store = live<number>("teams_count");
    const states: Live<number>[] = [];
    const first = store.subscribe(() => {});
    const firstOpen = await request("/__live/teams_count");
    first();
    unsubscribes.push(store.subscribe((s) => states.push(s)));
    const secondOpen = await request("/__live/teams_count");

    // The answer to the first open arrives late: it's stopped, not used.
    firstOpen.resolve(okResponse({ id: "old", value: 1 }));
    const stop = await request("/__live-stop");
    expect(decode(stop.init?.body as Uint8Array)).toBe("old");
    stop.resolve(okResponse(true));

    secondOpen.resolve(okResponse({ id: "new", value: 2 }));
    await vi.waitFor(() => expect(last(states)?.value).toBe(2));
  });
});
