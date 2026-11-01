/**
 * The inspector (RFC 0014): its routes, the window's route sent with the
 * window section, `can` honouring the mask, and nothing at all where the
 * build has no inspector.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { decode, encode } from "@msgpack/msgpack";
import {
  drainPump,
  errorResponse,
  headersOf,
  okResponse,
  stubFetch,
  stubPendingFetch,
  tick,
  type PendingCall,
} from "./test-support.js";

(globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };

const { can, inspector, openInspector } = await import("./index.js");

let pending: PendingCall[] = [];
let unsubscribes: Array<() => void> = [];

afterEach(async () => {
  await drainPump(pending, unsubscribes);
  pending = [];
  unsubscribes = [];
  vi.unstubAllGlobals();
  (globalThis as Record<string, unknown>).__ELYRA__ = { token: "test-token" };
});

const bodyOf = (init?: RequestInit) => decode(new Uint8Array(init!.body as ArrayBuffer)) as Record<string, unknown>;

describe("inspector", () => {
  it("lists the abilities, and masks one with the window's token and id", async () => {
    const rows = [{ ability: "customers.delete", granted: true, user: null, masked: true }];
    const calls = stubFetch(() => okResponse(rows));
    expect(await inspector.abilities()).toEqual(rows);
    expect(calls[0]!.url).toMatch(/\/__inspector\/abilities$/);
    expect(await inspector.mask("customers.delete", true)).toEqual(rows);
    expect(calls[1]!.url).toMatch(/\/__inspector\/mask$/);
    expect(bodyOf(calls[1]!.init)).toEqual({ ability: "customers.delete", off: true });
    const headers = headersOf(calls[1]!);
    expect(headers.get("x-elyra-token")).toBe("test-token");
    expect(headers.get("x-elyra-client-id")).toBeTruthy();
    await inspector.mask("customers.delete", false);
    expect(bodyOf(calls[2]!.init)).toEqual({ ability: "customers.delete", off: false });
  });

  it("asks for the window, the live queries and the calls", async () => {
    const calls = stubFetch((url) =>
      okResponse(url.endsWith("/window") ? { assistant: false } : []),
    );
    expect(await inspector.window()).toEqual({ assistant: false });
    expect(bodyOf(calls[0]!.init)).toEqual({ route: null });
    expect(await inspector.live()).toEqual([]);
    expect(await inspector.calls()).toEqual([]);
    expect(calls.map((c) => c.url.split("/__inspector/")[1])).toEqual(["window", "live", "calls"]);
  });

  it("unplugs the backend, plugs it back in, and asks without changing it", async () => {
    const state = { backend: true, on: true, copy: true };
    const calls = stubFetch(() => okResponse(state));
    expect(await inspector.offline(true)).toEqual(state);
    expect(calls[0]!.url).toMatch(/\/__inspector\/offline$/);
    expect(bodyOf(calls[0]!.init)).toEqual({ on: true });
    await inspector.offline(false);
    expect(bodyOf(calls[1]!.init)).toEqual({ on: false });
    await inspector.offline();
    expect(bodyOf(calls[2]!.init)).toEqual({});
  });

  it("asks for the presence channels", async () => {
    const seen = { enabled: true, me: "7", channels: [] };
    const calls = stubFetch(() => okResponse(seen));
    expect(await inspector.presence()).toEqual(seen);
    expect(calls[0]!.url).toMatch(/\/__inspector\/presence$/);
  });

  it("isn't there in a release build: available is false, and open does nothing", async () => {
    stubFetch(() => errorResponse("unknown route", "command", 404));
    expect(await inspector.available()).toBe(false);
    await openInspector();
  });

  it("is there in a debug build", async () => {
    stubFetch(() => okResponse([]));
    expect(await inspector.available()).toBe(true);
  });
});

describe("can, with the inspector's mask", () => {
  async function request(path: string): Promise<PendingCall> {
    for (;;) {
      await tick();
      const at = pending.findIndex((p) => p.url.endsWith(path));
      if (at >= 0) return pending.splice(at, 1)[0]!;
    }
  }

  it("says no to a masked ability, and its namespace, whatever the list says", async () => {
    pending = stubPendingFetch();
    let check: (ability: string) => boolean = () => false;
    unsubscribes.push(can.subscribe((c) => (check = c)));
    (await request("/__auth/abilities")).resolve(
      okResponse({ loaded: true, abilities: ["*"], masked: ["customers.delete", "invoices.*"] }),
    );
    await vi.waitFor(() => expect(check("customers.view")).toBe(true));
    expect(check("customers.delete")).toBe(false);
    expect(check("invoices.send")).toBe(false);

    // Unmasked in the inspector: Rust publishes the state without it.
    (await request("/__events")).resolve(
      new Response(encode([["elyra:abilities", { loaded: true, abilities: ["*"] }]]), { status: 200 }),
    );
    await vi.waitFor(() => expect(check("customers.delete")).toBe(true));
    expect(check("invoices.send")).toBe(true);
  });
});
