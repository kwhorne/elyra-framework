/**
 * Laravel's notifications in the runtime (RFC 0008): the `notifications`
 * store — read once, then followed — reading them, `onNotification`, and a
 * click in the OS opening an in-app path.
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

const { notifications, onNotification, onNotificationClicked, CommandError } = await import(
  "./index.js"
);

function batch(pairs: [string, unknown][]): Response {
  return new Response(encode(pairs), { status: 200 });
}

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
  vi.unstubAllGlobals();
});

/** The next pending request to `path`, taken out of `pending` in place. */
async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

const nextPoll = () => request("/__events");

const note = (id: string, readAt: string | null = null) => ({
  id,
  kind: "invoice-paid",
  data: { title: `Invoice ${id}`, url: `/invoices/${id}` },
  readAt,
  createdAt: "2026-10-13T10:00:00.000000Z",
});

const state = (unread: number, ids: string[]) => ({
  unread,
  items: ids.map((id) => note(id)),
  hasMore: false,
});

/** A window whose `location.hash` records where it was sent. */
function fakeWindow() {
  const went: string[] = [];
  vi.stubGlobal("location", {
    get hash() {
      return went[went.length - 1] ?? "";
    },
    set hash(value: string) {
      went.push(value);
    },
  });
  vi.stubGlobal("addEventListener", () => {});
  vi.stubGlobal("removeEventListener", () => {});
  return went;
}

describe("notifications", () => {
  it("is read once, then followed as they arrive and are read", async () => {
    pending = stubPendingFetch();
    const seen: number[] = [];
    unsubscribes.push(notifications.subscribe((s) => seen.push(s.unread)));
    expect(seen).toEqual([0]);

    (await request("/__auth/notifications")).resolve(okResponse(state(2, ["b", "a"])));
    await vi.waitFor(() => expect(seen[seen.length - 1]).toBe(2));

    (await nextPoll()).resolve(batch([["elyra:notifications", state(3, ["c", "b", "a"])]]));
    await vi.waitFor(() => expect(seen[seen.length - 1]).toBe(3));
  });

  it("reads and marks through its routes, and takes the answer", async () => {
    pending = stubPendingFetch();
    let unread = -1;
    unsubscribes.push(notifications.subscribe((s) => (unread = s.unread)));
    (await request("/__auth/notifications")).resolve(okResponse(state(2, ["a", "b"])));
    await vi.waitFor(() => expect(unread).toBe(2));

    const marked = notifications.markRead("a");
    const read = await request("/__auth/notifications-read");
    expect(decode(new Uint8Array(read.init!.body as Uint8Array))).toEqual({ id: "a" });
    read.resolve(okResponse(state(1, ["a", "b"])));
    await marked;
    expect(unread).toBe(1);

    for (const [op, call] of [
      ["notifications-read-all", () => notifications.markAllRead()],
      ["notifications-more", () => notifications.more()],
      ["notifications-refresh", () => notifications.refresh()],
    ] as const) {
      const done = call();
      (await request(`/__auth/${op}`)).resolve(okResponse(state(0, ["a"])));
      await done;
    }
    expect(unread).toBe(0);
  });

  it("passes the server's refusal on", async () => {
    stubFetch(() => errorResponse("The server is down.", "server"));
    const e = await rejection(notifications.markAllRead(), CommandError);
    expect(e.kind).toBe("server");
  });

  it("opens an in-app path a click asks for, and nothing else, while subscribed", async () => {
    const went = fakeWindow();
    pending = stubPendingFetch();
    const stop = notifications.subscribe(() => {});
    (await request("/__auth/notifications")).resolve(okResponse(state(0, [])));

    (await nextPoll()).resolve(
      batch([
        ["elyra:navigate", "https://evil.example/"],
        ["elyra:navigate", "//evil.example/x"],
        ["elyra:navigate", "/invoices/7"],
      ]),
    );
    await vi.waitFor(() => expect(went).toEqual(["#/invoices/7"]));

    // Unsubscribed: no longer followed.
    stop();
    const poll = pending.find((p) => p.url.endsWith("/__events"));
    poll?.resolve(batch([["elyra:navigate", "/customers"]]));
    await tick();
    await tick();
    expect(went).toEqual(["#/invoices/7"]);
  });
});

describe("onNotification", () => {
  it("hands over each one as it arrives, with where it was shown", async () => {
    pending = stubPendingFetch();
    const heard: string[] = [];
    unsubscribes.push(onNotification((e) => heard.push(`${e.presented.title}:${e.shown}`)));
    const clicked: string[] = [];
    unsubscribes.push(onNotificationClicked((e) => clicked.push(e.notification.id)));

    (await nextPoll()).resolve(
      batch([
        [
          "elyra:notification",
          {
            notification: note("9b1"),
            presented: { title: "Invoice paid", body: null, url: "/invoices/9b1" },
            shown: "toast",
          },
        ],
        [
          "elyra:notification-clicked",
          {
            notification: note("7"),
            presented: { title: "Invoice 7", body: null, url: "https://example.com" },
          },
        ],
      ]),
    );
    await vi.waitFor(() => expect(heard).toEqual(["Invoice paid:toast"]));
    await vi.waitFor(() => expect(clicked).toEqual(["7"]));
  });
});
