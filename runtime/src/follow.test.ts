/**
 * Following a colleague (RFC 0042): ask, be allowed, go where the leader
 * goes, and stop by going somewhere yourself; the leader's side says its
 * route only when its window has the focus, and only what it shares.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import { okResponse, stubPendingFetch, tick, type PendingCall } from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

// A window, before the runtime loads: following starts with one.
const target = new EventTarget();
let hash = "#/";
vi.stubGlobal("location", {
  get hash() {
    return hash;
  },
  set hash(value: string) {
    const next = value.startsWith("#") ? value : `#${value}`;
    if (next === hash) return;
    hash = next;
    target.dispatchEvent(new Event("hashchange"));
  },
});
vi.stubGlobal("addEventListener", target.addEventListener.bind(target));
vi.stubGlobal("removeEventListener", target.removeEventListener.bind(target));
let focused = true;
vi.stubGlobal("document", { hasFocus: () => focused, addEventListener: () => {}, removeEventListener: () => {} });

const { presence, following, followers, navigate } = await import("./index.js");
type Following = import("./index.js").Following;
type Follower = import("./index.js").Follower;

let pending: PendingCall[] = [];
afterEach(() => {
  for (const p of pending.splice(0)) p.resolve(okResponse(null));
});

async function request(path: string): Promise<PendingCall> {
  for (;;) {
    await tick();
    const at = pending.findIndex((p) => p.url.endsWith(path));
    if (at >= 0) return pending.splice(at, 1)[0]!;
  }
}

const body = (call: PendingCall) => decode(new Uint8Array(call.init!.body as Uint8Array));
const event = async (name: string, payload: unknown) =>
  (await request("/__events")).resolve(new Response(encode([[name, payload]]), { status: 200 }));

describe("following", () => {
  it("asks, is allowed, goes where the leader goes, and stops on its own navigation", async () => {
    pending = stubPendingFetch();
    let now: Following | null = null;
    let them: Follower[] = [];
    following.subscribe((f) => (now = f));
    followers.subscribe((f) => (them = f));
    (await request("/__presence/follow-state")).resolve(okResponse({ following: null, followers: [] }));

    const allowed = presence("lobby").follow("9");
    const ask = await request("/__presence/follow");
    expect(body(ask)).toEqual({ channel: "lobby", user: "9" });
    ask.resolve(okResponse({ following: { channel: "lobby", leader: "9", allowed: false } }));
    await event("elyra:follow", { kind: "following", channel: "lobby", leader: "9" });
    expect(await allowed).toBe(true);
    expect(now).toEqual({ channel: "lobby", leader: "9", allowed: true });

    await event("elyra:follow", { kind: "route", route: "/customers/42" });
    await vi.waitFor(() => expect(hash).toBe("#/customers/42"));
    // Going there doesn't stop it, and nothing is said as this user's route…
    await event("elyra:follow", { kind: "route", route: null });
    await vi.waitFor(() => expect(now?.away).toBe(true));

    // …but going somewhere yourself does.
    navigate("/invoices");
    const stop = await request("/__presence/unfollow");
    stop.resolve(okResponse({ following: null, followers: [] }));
    await vi.waitFor(() => expect(now).toBeNull());

    await event("elyra:follow", { kind: "followers", followers: [{ channel: "lobby", id: "12" }] });
    await vi.waitFor(() => expect(them).toEqual([{ channel: "lobby", id: "12" }]));
  });

  it("says this user's route while focused, and only what it shares", async () => {
    pending = stubPendingFetch();
    focused = true;
    following.share((route) => !route.startsWith("/settings"));
    // Sharing says the route again, as it stands now.
    (await request("/__presence/route")).resolve(okResponse(true));
    navigate("/settings/profile");
    let said = await request("/__presence/route");
    expect(body(said)).toEqual({ route: null });
    said.resolve(okResponse(true));
    navigate("/customers/7");
    said = await request("/__presence/route");
    expect(body(said)).toEqual({ route: "/customers/7" });
    said.resolve(okResponse(true));
    focused = false;
    navigate("/customers/8");
    await tick();
    expect(pending.some((p) => p.url.endsWith("/__presence/route"))).toBe(false);
  });
});
