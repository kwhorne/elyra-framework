/**
 * Working offline in the runtime (RFC 0007): `connection` is the backend's
 * state, `outbox` the writes that wait — read once, then followed — and
 * signing out with writes waiting is the user's choice to make.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import {
  drainPump,
  errorResponse,
  okResponse,
  rejection,
  stubFetch,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { connection, outbox, auth, CommandError } = await import("./index.js");

function batch(pairs: [string, unknown][]): Response {
  return new Response(encode(pairs), { status: 200 });
}

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
  vi.unstubAllGlobals();
});

/** The next pending request to `path`, taken out of `pending` (in place: the
 * fetch stub keeps adding to the same array). */
async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

const nextPoll = () => request("/__events");

const item = (id: number, status: "pending" | "failed" = "pending") => ({
  id,
  method: "POST",
  path: "/api/customers",
  body: { name: `Customer ${id}` },
  status,
  attempts: 0,
  error: null,
  errors: null,
  reason: null,
  createdAt: 1,
});

describe("connection", () => {
  it("is online until asked, then the backend's state, followed", async () => {
    pending = stubPendingFetch();
    const seen: string[] = [];
    unsubscribes.push(connection.subscribe((s) => seen.push(`${s.state}:${s.pending}`)));
    expect(seen).toEqual(["online:0"]);

    (await request("/__auth/connection")).resolve(
      okResponse({ state: "offline", since: 1000, pending: 2, failed: 0 }),
    );
    await vi.waitFor(() => expect(seen[seen.length - 1]).toBe("offline:2"));

    (await nextPoll()).resolve(
      batch([["elyra:connection", { state: "online", since: null, pending: 0, failed: 0 }]]),
    );
    await vi.waitFor(() => expect(seen[seen.length - 1]).toBe("online:0"));
  });
});

describe("outbox", () => {
  it("is read once, and again whenever it changes", async () => {
    pending = stubPendingFetch();
    let items: unknown[] = [];
    unsubscribes.push(outbox.subscribe((i) => (items = i)));

    (await request("/__auth/outbox")).resolve(okResponse([item(1)]));
    await vi.waitFor(() => expect(items).toHaveLength(1));

    // One sent, one kept: the counts don't change, the list does.
    (await nextPoll()).resolve(batch([["elyra:outbox", { pending: 1, failed: 0 }]]));
    (await request("/__auth/outbox")).resolve(okResponse([item(2)]));
    await vi.waitFor(() => expect((items[0] as { id: number }).id).toBe(2));
  });

  it("retries with a fixed body, and resolves conflicts", async () => {
    const calls = stubFetch((url) =>
      url.endsWith("/__events") ? batch([]) : okResponse([item(3, "failed")]),
    );
    await outbox.retry(3, { name: "Fixed" });
    await outbox.keepMine(3);
    await outbox.takeTheirs(3);
    await outbox.discard(3);
    const ops = calls
      .filter((c) => c.url.includes("/__auth/"))
      .map((c) => c.url.split("/__auth/")[1]);
    expect(ops).toEqual(["outbox-retry", "outbox-keep-mine", "outbox-take-theirs", "outbox-discard"]);
    const sent = decode(
      new Uint8Array(calls.find((c) => c.url.endsWith("outbox-retry"))!.init!.body as Uint8Array),
    );
    expect(sent).toEqual({ id: 3, body: { name: "Fixed" } });
  });

  it("passes the server's refusal on", async () => {
    stubFetch(() => errorResponse("not found", "not-found"));
    const e = await rejection(outbox.discard(9), CommandError);
    expect(e.kind).toBe("not-found");
  });
});

describe("signing out with writes waiting", () => {
  it("is refused with pending_changes, until the user discards them", async () => {
    const calls = stubFetch((url) =>
      url.endsWith("/sign-out")
        ? errorResponse("2 change(s) haven't been sent yet", "pending_changes")
        : okResponse({ signedIn: false, user: null, reason: "signed-out" }),
    );
    const e = await rejection(auth.signOut(), CommandError);
    expect(e.kind).toBe("pending_changes");

    const state = await auth.signOutDiscarding();
    expect(state.signedIn).toBe(false);
    expect(calls[calls.length - 1]!.url).toMatch(/\/__auth\/sign-out-discarding$/);
  });
});
