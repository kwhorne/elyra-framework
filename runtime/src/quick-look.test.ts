/**
 * Quick Look (RFC 0041): the page asks for one of the app's routes, and
 * `$quickLook` follows what the shell says is shown.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import {
  drainPump,
  okResponse,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { quickLook } = await import("./index.js");
type QuickLookState = import("./index.js").QuickLookState;

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

describe("quickLook", () => {
  it("shows a route through the window route, and hides", async () => {
    pending = stubPendingFetch();
    const shown = quickLook.show("/customers/42");
    const call = await request("/__window/quick-look-show");
    expect(decode(new Uint8Array(call.init!.body as Uint8Array))).toBe("/customers/42");
    call.resolve(okResponse(true));
    await shown;

    const hidden = quickLook.hide();
    (await request("/__window/quick-look-hide")).resolve(okResponse(true));
    await hidden;
  });

  it("follows what the shell says is shown", async () => {
    pending = stubPendingFetch();
    let seen: QuickLookState = { open: false, route: null };
    unsubscribes.push(quickLook.subscribe((q) => (seen = q)));
    expect(seen).toEqual({ open: false, route: null });
    (await request("/__events")).resolve(
      new Response(encode([["elyra:quick-look", { open: true, route: "/customers/42" }]]), {
        status: 200,
      }),
    );
    await vi.waitFor(() => expect(seen).toEqual({ open: true, route: "/customers/42" }));
  });

  it("knows the window it's in", () => {
    expect(quickLook.here()).toBe(false);
  });
});
